#[cfg(test)]
fn transcribe_timeout(encoded_bytes: usize) -> Duration {
    PRODUCTION_TIMEOUTS.transcribe_timeout(encoded_bytes)
}

fn terminate_child(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn spawn_stdin_writer(stdin: ChildStdin) -> (Sender<WorkerWrite>, JoinHandle<()>) {
    let (sender, receiver) = bounded::<WorkerWrite>(1);
    let thread = thread::spawn(move || {
        let mut stdin = stdin;
        while let Ok(write) = receiver.recv() {
            let result = writeln!(stdin, "{}", write.line)
                .and_then(|_| stdin.flush())
                .map_err(|error| error.to_string());
            let failed = result.is_err();
            let _ = write.result_tx.try_send(result);
            if failed {
                break;
            }
        }
    });
    (sender, thread)
}

fn spawn_stdout_reader(stdout: ChildStdout) -> (Receiver<WorkerOutput>, JoinHandle<()>) {
    let (sender, receiver) = bounded(8);
    let thread = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let output = match read_limited_line(&mut reader, MAX_RESPONSE_FRAME_BYTES) {
                Ok(Some(line)) => WorkerOutput::Line(line),
                Ok(None) => WorkerOutput::Eof,
                Err(error) => WorkerOutput::Error(error.to_string()),
            };
            let terminal = !matches!(output, WorkerOutput::Line(_));
            if sender.try_send(output).is_err() || terminal {
                break;
            }
        }
    });
    (receiver, thread)
}

fn spawn_stderr_reader(stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        loop {
            let line = match read_limited_line(&mut reader, MAX_STDERR_LINE_BYTES) {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(error) => {
                    let mut tail = tail.lock();
                    push_stderr_line(&mut tail, format!("<stderr reader error: {error}>"));
                    continue;
                }
            };
            push_stderr_line(&mut tail.lock(), line);
        }
    })
}

fn push_stderr_line(tail: &mut VecDeque<String>, line: String) {
    if tail.len() == STDERR_TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line);
}

fn read_limited_line<R: BufRead>(
    reader: &mut R,
    maximum_bytes: usize,
) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut exceeded_limit = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() {
                if exceeded_limit {
                    return Err(std::io::Error::new(
                        ErrorKind::InvalidData,
                        format!("worker line exceeds {maximum_bytes} bytes"),
                    ));
                }
                return Ok(None);
            }
            break;
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !exceeded_limit && bytes.len().saturating_add(consumed) > maximum_bytes {
            exceeded_limit = true;
            bytes.clear();
        }
        if !exceeded_limit {
            bytes.extend_from_slice(&available[..consumed]);
        }
        reader.consume(consumed);
        if newline.is_some() {
            if exceeded_limit {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    format!("worker line exceeds {maximum_bytes} bytes"),
                ));
            }
            break;
        }
    }

    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))
}

fn backend_name(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::Cuda => "CUDA",
        BackendKind::Vulkan => "Vulkan",
        BackendKind::Cpu => "CPU",
    }
}
