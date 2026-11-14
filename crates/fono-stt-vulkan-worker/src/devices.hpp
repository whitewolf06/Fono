#pragma once
#include "json.hpp"
#include <vulkan/vulkan.h>
#include <algorithm>
#include <cstdlib>
#include <cstring>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

// Enumerate physical adapters without loading a model or creating a GPU device.
// In particular, a Vulkan CPU driver must never masquerade as GPU acceleration.
struct VulkanAdapter {
    uint32_t index;
    std::string name, uuid, driver;
    bool discrete, supported;
    uint64_t total;
    std::optional<uint64_t> free;
};
struct VulkanDriverUnavailable : std::runtime_error { using std::runtime_error::runtime_error; };
inline std::vector<VulkanAdapter> vulkan_adapters() {
    VkApplicationInfo app{VK_STRUCTURE_TYPE_APPLICATION_INFO};
    app.pApplicationName = "Fono";
    app.apiVersion = VK_API_VERSION_1_2;
    VkInstanceCreateInfo create{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
    create.pApplicationInfo = &app;
    VkInstance instance = VK_NULL_HANDLE;
    if (vkCreateInstance(&create, nullptr, &instance) != VK_SUCCESS) {
        throw VulkanDriverUnavailable("Vulkan driver is unavailable or does not support Vulkan 1.2");
    }
    struct InstanceGuard { VkInstance value; ~InstanceGuard() { vkDestroyInstance(value, nullptr); } } guard{instance};
    uint32_t count = 0;
    if (vkEnumeratePhysicalDevices(instance, &count, nullptr) != VK_SUCCESS || count > 64) {
        throw std::runtime_error("Cannot enumerate Vulkan adapters");
    }
    std::vector<VkPhysicalDevice> physical(count);
    if (count && vkEnumeratePhysicalDevices(instance, &count, physical.data()) != VK_SUCCESS) {
        throw std::runtime_error("Vulkan adapter inventory changed");
    }
    std::vector<VulkanAdapter> adapters;
    constexpr char hex[] = "0123456789abcdef";
    for (uint32_t i = 0; i < count; ++i) {
        VkPhysicalDeviceIDProperties id{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
        VkPhysicalDeviceDriverProperties driver{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES};
        id.pNext = &driver;
        VkPhysicalDeviceProperties2 props{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2};
        props.pNext = &id;
        vkGetPhysicalDeviceProperties2(physical[i], &props);
        VkPhysicalDevice16BitStorageFeatures storage{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_16BIT_STORAGE_FEATURES};
        VkPhysicalDeviceFeatures2 features{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};
        features.pNext = &storage;
        vkGetPhysicalDeviceFeatures2(physical[i], &features);
        uint32_t queue_count = 0;
        vkGetPhysicalDeviceQueueFamilyProperties(physical[i], &queue_count, nullptr);
        std::vector<VkQueueFamilyProperties> queues(queue_count);
        vkGetPhysicalDeviceQueueFamilyProperties(physical[i], &queue_count, queues.data());
        const bool compute = std::any_of(queues.begin(), queues.end(), [](const auto & q) { return q.queueCount && (q.queueFlags & VK_QUEUE_COMPUTE_BIT); });
        uint32_t extension_count = 0;
        bool budget_supported = false;
        if (vkEnumerateDeviceExtensionProperties(physical[i], nullptr, &extension_count, nullptr) == VK_SUCCESS && extension_count <= 4096) {
            std::vector<VkExtensionProperties> extensions(extension_count);
            if (vkEnumerateDeviceExtensionProperties(physical[i], nullptr, &extension_count, extensions.data()) == VK_SUCCESS) {
                budget_supported = std::any_of(extensions.begin(), extensions.end(), [](const auto & e) { return std::strcmp(e.extensionName, VK_EXT_MEMORY_BUDGET_EXTENSION_NAME) == 0; });
            }
        }
        VkPhysicalDeviceMemoryBudgetPropertiesEXT budget{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MEMORY_BUDGET_PROPERTIES_EXT};
        VkPhysicalDeviceMemoryProperties2 memory{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MEMORY_PROPERTIES_2};
        memory.pNext = budget_supported ? &budget : nullptr;
        vkGetPhysicalDeviceMemoryProperties2(physical[i], &memory);
        uint64_t total = 0, free = 0;
        for (uint32_t heap = 0; heap < memory.memoryProperties.memoryHeapCount; ++heap) {
            if (!(memory.memoryProperties.memoryHeaps[heap].flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT)) { continue; }
            total += memory.memoryProperties.memoryHeaps[heap].size;
            if (budget_supported) { free += budget.heapBudget[heap] > budget.heapUsage[heap] ? budget.heapBudget[heap] - budget.heapUsage[heap] : 0; }
        }
        std::string uuid;
        for (const auto byte : id.deviceUUID) { uuid += hex[byte >> 4]; uuid += hex[byte & 15]; }
        const bool supported = props.properties.apiVersion >= VK_API_VERSION_1_2 && storage.storageBuffer16BitAccess && compute;
        adapters.push_back({i, props.properties.deviceName, uuid, driver.driverName,
            props.properties.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU, supported, total,
            budget_supported ? std::optional<uint64_t>(std::min(free, total)) : std::nullopt});
    }
    return adapters;
}
inline nlohmann::json adapter_inventory() {
    auto devices = nlohmann::json::array();
    for (const auto & d : vulkan_adapters()) {
        devices.push_back({{"index", d.index}, {"name", d.name}, {"uuid", d.uuid}, {"driver", d.driver},
            {"discrete", d.discrete}, {"supported", d.supported}, {"memory_total_bytes", d.total},
            // EXT_memory_budget describes this process's allocation budget,
            // not the adapter-wide free VRAM. Preserve the distinction.
            {"memory_free_bytes", nullptr},
            {"memory_budget_free_bytes", d.free ? nlohmann::json(*d.free) : nlohmann::json(nullptr)}});
    }
    return {{"schema_version", 1}, {"devices", devices}, {"driver_available", true},
        {"gpu_policy", "discrete_only"}, {"load_warmup", {"classic", "windowed"}}};
}
struct NoVulkanGpu : std::runtime_error { using std::runtime_error::runtime_error; };
inline VulkanAdapter select_vulkan_adapter() {
    const auto adapters = vulkan_adapters();
    const char * selection = std::getenv("GGML_VK_VISIBLE_DEVICES");
    if (selection) {
        const std::string value(selection);
        for (const auto & d : adapters) {
            if (value == std::to_string(d.index)) {
                if (!d.discrete || !d.supported) { throw NoVulkanGpu("Selected Vulkan device is not a supported discrete GPU"); }
                return d;
            }
        }
        throw std::runtime_error("Select one canonical physical Vulkan device index");
    }
    for (const auto & d : adapters) { if (d.discrete && d.supported) { return d; } }
    throw NoVulkanGpu("No supported discrete Vulkan GPU is available");
}
inline void pin_vulkan_adapter(const VulkanAdapter & device) {
    const auto value = std::to_string(device.index);
#ifdef _WIN32
    if (_putenv_s("GGML_VK_VISIBLE_DEVICES", value.c_str()) != 0) { throw std::runtime_error("Cannot select Vulkan GPU"); }
#else
    if (setenv("GGML_VK_VISIBLE_DEVICES", value.c_str(), 1) != 0) { throw std::runtime_error("Cannot select Vulkan GPU"); }
#endif
}
