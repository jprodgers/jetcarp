//! A GPU's memory as its Vulkan driver gives it, on Linux, for a GPU whose memory nothing else
//! gives ([`super::hardware::read_drm`]): nouveau's NVIDIA GPUs (with Mesa's NVK), i915's
//! discrete Intel GPUs, NVIDIA's without NVML. Its memory is the largest heap the driver marks
//! as the device's own (`VK_MEMORY_HEAP_DEVICE_LOCAL_BIT`, from
//! `vkGetPhysicalDeviceMemoryProperties`), found among the DRM cards by its PCI address
//! (`VK_EXT_pci_bus_info`). An integrated GPU's device-local heap is the system's memory, so it
//! counts as memory the GPU may borrow, not its own.
//!
//! The system's Vulkan loader (`libvulkan.so.1`) is opened while running through `ash`, which
//! wgpu already uses, so this adds no build dependency; it is opened only when a GPU needs it,
//! and closed again at once.

use super::hardware::PciAddress;

/// A GPU as its Vulkan driver describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VulkanGpu {
    /// Its PCI address (`VK_EXT_pci_bus_info`), where the driver gives it.
    pub address: Option<PciAddress>,
    /// Its PCI vendor and device ids.
    pub vendor: u32,
    pub device: u32,
    /// `VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU`: its memory is the system's.
    pub integrated: bool,
    /// Each memory heap's size in bytes, and whether it is the device's own (`DEVICE_LOCAL`).
    pub heaps: Vec<(u64, bool)>,
}

impl VulkanGpu {
    /// Its largest device-local heap, in bytes.
    pub fn device_local(&self) -> Option<u64> {
        self.heaps
            .iter()
            .filter(|(_, own)| *own)
            .map(|(b, _)| *b)
            .max()
    }
}

/// Every GPU the system's Vulkan drivers list, but software renderers (llvmpipe). None where
/// there is no Vulkan loader or driver.
#[cfg(target_os = "linux")]
pub fn gpus() -> Vec<VulkanGpu> {
    use ash::vk;
    // SAFETY: opens the system's Vulkan loader (libvulkan.so.1); an error means there is none.
    let Ok(entry) = (unsafe { ash::Entry::load() }) else {
        return vec![];
    };
    // A device's PCI address is asked with vkGetPhysicalDeviceProperties2, of Vulkan 1.1.
    // SAFETY: the loader is open.
    let v11 = matches!(
        unsafe { entry.try_enumerate_instance_version() },
        Ok(Some(v)) if v >= vk::API_VERSION_1_1
    );
    let app = vk::ApplicationInfo::default()
        .application_name(c"JETCARP")
        .api_version(if v11 {
            vk::API_VERSION_1_1
        } else {
            vk::API_VERSION_1_0
        });
    let info = vk::InstanceCreateInfo::default().application_info(&app);
    // SAFETY: a plain instance, no layers or extensions; destroyed below.
    let Ok(instance) = (unsafe { entry.create_instance(&info, None) }) else {
        return vec![];
    };
    let mut out = vec![];
    // SAFETY: each call is on the live instance and a device it listed, and writes only the
    // structures it is given; the instance is destroyed once, after the last of them.
    unsafe {
        for pd in instance.enumerate_physical_devices().unwrap_or_default() {
            let p = instance.get_physical_device_properties(pd);
            if p.device_type == vk::PhysicalDeviceType::CPU {
                continue;
            }
            let pci_ext = instance
                .enumerate_device_extension_properties(pd)
                .is_ok_and(|e| {
                    e.iter()
                        .any(|x| x.extension_name_as_c_str() == Ok(ash::ext::pci_bus_info::NAME))
                });
            let address = (v11 && p.api_version >= vk::API_VERSION_1_1 && pci_ext).then(|| {
                let mut pci = vk::PhysicalDevicePCIBusInfoPropertiesEXT::default();
                let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut pci);
                instance.get_physical_device_properties2(pd, &mut p2);
                (
                    pci.pci_domain,
                    pci.pci_bus,
                    pci.pci_device,
                    pci.pci_function,
                )
            });
            let m = instance.get_physical_device_memory_properties(pd);
            out.push(VulkanGpu {
                address,
                vendor: p.vendor_id,
                device: p.device_id,
                integrated: p.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU,
                heaps: m
                    .memory_heaps_as_slice()
                    .iter()
                    .map(|h| (h.size, h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL)))
                    .collect(),
            });
        }
        instance.destroy_instance(None);
    }
    out.retain(|g| g.device_local().is_some());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    #[test]
    fn the_largest_device_local_heap_is_the_gpu_s_memory() {
        // A discrete GPU on NVK: its memory, the system's, and the part of its memory the
        // processor reaches through the PCI bar (device-local too, but smaller).
        let nvk = VulkanGpu {
            heaps: vec![(5_900 << 20, true), (16 * GIB, false), (256 << 20, true)],
            ..Default::default()
        };
        assert_eq!(nvk.device_local(), Some(5_900 << 20));
        // No device-local heap, or none at all: nothing.
        let none = VulkanGpu {
            heaps: vec![(16 * GIB, false)],
            ..Default::default()
        };
        assert_eq!(none.device_local(), None);
        assert_eq!(VulkanGpu::default().device_local(), None);
    }

    /// The Vulkan drivers here, read: each GPU with a device-local heap. On Linux with an AMD
    /// GPU whose driver gives its PCI address (RADV does), that GPU is listed at the address its
    /// DRM card has. CI's runners have only llvmpipe, which is left out.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_vulkan_gpus_here() {
        let t = std::time::Instant::now();
        let g = gpus();
        eprintln!("Vulkan GPUs here, read in {:?}: {g:?}", t.elapsed());
        assert!(g.iter().all(|g| g.device_local().is_some()), "{g:?}");
        let amd = std::fs::read_dir("/sys/class/drm")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let dev = e.path().join("device");
                let ue = std::fs::read_to_string(dev.join("uevent")).ok()?;
                (ue.contains("DRIVER=amdgpu") && dev.join("mem_info_vram_total").exists())
                    .then_some(())?;
                ue.lines()
                    .find_map(|l| l.strip_prefix("PCI_SLOT_NAME="))
                    .and_then(super::super::hardware::parse_pci_address)
            })
            .next();
        if let Some(a) = amd
            && std::path::Path::new(&format!(
                "/usr/share/vulkan/icd.d/radeon_icd.{}.json",
                std::env::consts::ARCH
            ))
            .exists()
        {
            assert!(
                g.iter().any(|g| g.address == Some(a) && g.vendor == 0x1002),
                "the Radeon at {a:x?} is not among {g:?}"
            );
        }
    }
}
