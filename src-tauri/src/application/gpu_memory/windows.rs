use super::policy::MemorySample;
use windows::core::{w, PCWSTR};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PdhSetCounterScaleFactor, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_MORE_DATA,
};

const MAX_COUNTER_BYTES: u32 = 1024 * 1024;
const MAX_INSTANCE_CHARS: usize = 512;
const MIN_DEDICATED_BYTES: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AdapterMemory {
    luid: (u32, u32),
    total_bytes: u64,
}

struct Query(isize);

impl Drop for Query {
    fn drop(&mut self) {
        // Closing the query also releases every counter registered with it.
        unsafe { PdhCloseQuery(self.0) };
    }
}

/// Global dedicated-memory telemetry for one unambiguous hardware adapter.
/// Shared system memory and per-process usage are deliberately not included.
pub(super) struct Monitor {
    query: Query,
    counter: isize,
    adapter: AdapterMemory,
}

impl Monitor {
    pub(super) fn new() -> Result<Self, &'static str> {
        let adapter = only_adapter(&hardware_adapters()?)?;
        let mut handle = 0;
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut handle) } != 0 {
            return Err("Счётчики видеопамяти Windows недоступны.");
        }
        let query = Query(handle);
        let mut counter = 0;
        let status = unsafe {
            PdhAddEnglishCounterW(
                query.0,
                w!("\\GPU Adapter Memory(*)\\Dedicated Usage"),
                0,
                &mut counter,
            )
        };
        if status != 0 || unsafe { PdhSetCounterScaleFactor(counter, 0) } != 0 {
            return Err("Счётчик выделенной видеопамяти недоступен.");
        }
        if unsafe { PdhCollectQueryData(query.0) } != 0 {
            return Err("Windows не предоставила данные видеопамяти.");
        }
        Ok(Self {
            query,
            counter,
            adapter,
        })
    }

    pub(super) fn sample(&mut self) -> Result<MemorySample, &'static str> {
        // GPU topology can change while a worker is idle. Do not keep interpreting
        // an old adapter's pressure as belonging to a new default backend device.
        if only_adapter(&hardware_adapters()?)? != self.adapter {
            return Err("Графический адаптер изменился; точное измерение недоступно.");
        }
        if unsafe { PdhCollectQueryData(self.query.0) } != 0 {
            return Err("Не удалось обновить данные видеопамяти.");
        }
        let (buffer, count) = counter_array(self.counter)?;
        let mut selected = None;
        for index in 0..count {
            // The u64 buffer provides alignment for both the structures and UTF-16 names.
            // counter_array bounds-checks the number of complete structures first.
            let item = unsafe {
                buffer
                    .as_ptr()
                    .cast::<PDH_FMT_COUNTERVALUE_ITEM_W>()
                    .add(index)
                    .read()
            };
            let name = instance_name(&buffer, item.szName.0)
                .ok_or("Windows вернула некорректное имя счётчика видеопамяти.")?;
            let Some((luid, physical)) = parse_instance(&name) else {
                continue;
            };
            if luid != self.adapter.luid {
                continue;
            }
            if physical != 0 || selected.is_some() {
                return Err("Несколько узлов GPU: точное измерение пока недоступно.");
            }
            if !matches!(
                item.FmtValue.CStatus,
                PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
            ) {
                return Err("Данные видеопамяти временно недоступны.");
            }
            selected = Some(unsafe { item.FmtValue.Anonymous.largeValue });
        }
        let used = selected.ok_or("Не найден счётчик выбранного GPU.")?;
        memory_sample(used, self.adapter.total_bytes)
    }
}

fn hardware_adapters() -> Result<Vec<AdapterMemory>, &'static str> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
        .map_err(|_| "Windows не предоставила список графических адаптеров.")?;
    let mut result = Vec::new();
    for index in 0..256 {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return Ok(result),
            Err(_) => return Err("Не удалось определить графический адаптер."),
        };
        let description = unsafe { adapter.GetDesc1() }
            .map_err(|_| "Не удалось прочитать параметры графического адаптера.")?;
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        result.push(AdapterMemory {
            luid: (
                description.AdapterLuid.HighPart as u32,
                description.AdapterLuid.LowPart,
            ),
            total_bytes: description.DedicatedVideoMemory as u64,
        });
    }
    Err("Список графических адаптеров не удалось определить однозначно.")
}

fn only_adapter(adapters: &[AdapterMemory]) -> Result<AdapterMemory, &'static str> {
    match adapters {
        [adapter] if adapter.total_bytes >= MIN_DEDICATED_BYTES => Ok(*adapter),
        [] => Err("Аппаратный GPU недоступен."),
        [_] => Err("Мониторинг доступен для видеокарты с выделенной памятью от 1 ГБ."),
        _ => Err("Несколько GPU: автоматическая проверка видеопамяти пока недоступна."),
    }
}

fn counter_array(counter: isize) -> Result<(Vec<u64>, usize), &'static str> {
    // Re-probe from zero if instances change between the sizing and data calls.
    for _ in 0..3 {
        let mut bytes = 0;
        let mut count = 0;
        let status = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut bytes, &mut count, None)
        };
        if status != PDH_MORE_DATA || bytes == 0 || bytes > MAX_COUNTER_BYTES {
            return Err("Данные счётчиков GPU недоступны или имеют неверный размер.");
        }
        let mut buffer = vec![0_u64; (bytes as usize).div_ceil(size_of::<u64>())];
        let capacity_bytes = buffer.len() * size_of::<u64>();
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                &mut bytes,
                &mut count,
                Some(buffer.as_mut_ptr().cast()),
            )
        };
        if status == PDH_MORE_DATA {
            continue;
        }
        let headers_bytes = (count as usize)
            .checked_mul(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>())
            .ok_or("Некорректное количество счётчиков GPU.")?;
        if status != 0 || bytes as usize > capacity_bytes || headers_bytes > bytes as usize {
            return Err("Windows не предоставила корректные счётчики GPU.");
        }
        return Ok((buffer, count as usize));
    }
    Err("Список счётчиков GPU изменился; проверка будет повторена позже.")
}

fn instance_name(buffer: &[u64], name: *const u16) -> Option<String> {
    let start = buffer.as_ptr() as usize;
    let end = start.checked_add(size_of_val(buffer))?;
    let address = name as usize;
    if address < start || address >= end || address % align_of::<u16>() != 0 {
        return None;
    }
    let max_chars = ((end - address) / size_of::<u16>()).min(MAX_INSTANCE_CHARS);
    let mut chars = Vec::new();
    for offset in 0..max_chars {
        // The pointer and each UTF-16 unit are inside the owned, aligned allocation.
        let value = unsafe { name.add(offset).read() };
        if value == 0 {
            return String::from_utf16(&chars).ok();
        }
        chars.push(value);
    }
    None
}

fn parse_instance(name: &str) -> Option<((u32, u32), u32)> {
    let mut parts = name.strip_prefix("luid_")?.split('_');
    let high = u32::from_str_radix(parts.next()?.strip_prefix("0x")?, 16).ok()?;
    let low = u32::from_str_radix(parts.next()?.strip_prefix("0x")?, 16).ok()?;
    if parts.next()? != "phys" {
        return None;
    }
    let physical = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(((high, low), physical))
}

fn memory_sample(used: i64, total: u64) -> Result<MemorySample, &'static str> {
    let used_bytes = u64::try_from(used).map_err(|_| "Некорректный расход видеопамяти.")?;
    if total == 0 || used_bytes > total {
        return Err("Расход и объём видеопамяти не удалось сопоставить.");
    }
    Ok(MemorySample {
        used_bytes,
        total_bytes: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_luid_matches_hex_halves_without_relying_on_gpu_order() {
        assert_eq!(
            parse_instance("luid_0xFFFFFFFF_0x00012aBc_phys_0"),
            Some(((u32::MAX, 0x12abc), 0))
        );
        assert!(parse_instance("pid_42_luid_0x00000000_0x00000001_phys_0").is_none());
        assert!(parse_instance("luid_0x0_0x1_phys_0_extra").is_none());
    }

    #[test]
    fn integrated_adapter_counts_even_without_dedicated_memory() {
        let discrete = AdapterMemory {
            luid: (0, 1),
            total_bytes: 8 << 30,
        };
        let integrated = AdapterMemory {
            luid: (0, 2),
            total_bytes: 0,
        };
        assert!(only_adapter(&[discrete, integrated]).is_err());
        assert!(only_adapter(&[integrated]).is_err());
        assert!(only_adapter(&[AdapterMemory {
            total_bytes: 128 << 20,
            ..integrated
        }])
        .is_err());
        assert_eq!(only_adapter(&[discrete]).unwrap(), discrete);
    }

    #[test]
    fn absent_or_invalid_samples_do_not_become_free_memory() {
        assert!(memory_sample(-1, 4096).is_err());
        assert!(memory_sample(4097, 4096).is_err());
        assert!(memory_sample(0, 0).is_err());
        assert_eq!(memory_sample(0, 4096).unwrap().used_bytes, 0);
    }

    #[test]
    fn names_are_decoded_only_inside_the_counter_allocation() {
        let mut buffer = vec![0_u64; 4];
        let name = buffer.as_mut_ptr().cast::<u16>();
        unsafe { name.write(b'x' as u16) };
        assert_eq!(instance_name(&buffer, name), Some("x".into()));
        assert!(instance_name(&buffer, std::ptr::null()).is_none());
        assert!(instance_name(&buffer, name.wrapping_add(16)).is_none());
    }
}
