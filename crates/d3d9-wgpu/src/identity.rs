//! The graphics card's identity as wgpu reports it (fork-owned): the input of the Linux machine
//! table (`engine-platform`'s `fork_tables`), which maps it onto the Direct3D 9 identity.

/// What wgpu's `AdapterInfo` says about a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuIdentity {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// The driver's name (`radv`) and its description, as the Vulkan driver gives them.
    pub driver: String,
    pub driver_info: String,
    /// A software adapter (lavapipe), the one a machine without a card has.
    pub software: bool,
}

/// The first graphics card wgpu finds (a Vulkan, Metal or DX12 one), else the first software
/// adapter, else none. No window is needed.
pub fn gpu_identity() -> Option<GpuIdentity> {
    let backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let infos: Vec<_> = crate::device::block_on(instance.enumerate_adapters(backends))
        .into_iter()
        .map(|a| a.get_info())
        .collect();
    let pick = infos
        .iter()
        .find(|i| i.device_type != wgpu::DeviceType::Cpu)
        .or_else(|| infos.first())?;
    Some(GpuIdentity {
        name: pick.name.clone(),
        vendor_id: pick.vendor,
        device_id: pick.device,
        driver: pick.driver.clone(),
        driver_info: pick.driver_info.clone(),
        software: pick.device_type == wgpu::DeviceType::Cpu,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where an adapter exists (the CI shell's lavapipe, a developer's card) it has a name and a
    /// vendor; a machine with none answers none and the test has nothing to check.
    #[test]
    fn the_adapter_wgpu_finds_has_an_identity() {
        if let Some(id) = gpu_identity() {
            assert!(!id.name.is_empty());
            assert_ne!(id.vendor_id, 0, "{id:?}");
        }
    }
}
