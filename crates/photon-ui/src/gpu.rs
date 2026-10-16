//! Which graphics adapter draws the window, and the fallback where none of the machine's
//! own can (spec `2026-10-16-photon-native-platform-design.md`, decision 6; the user's
//! answer: wgpu's own software adapter - WARP on Windows, llvmpipe on Linux - and a plain
//! message where even that fails, no second renderer).
//!
//! wgpu's own choice already takes a software adapter, last, where it is the only one that
//! can draw to the window (`wgpu_core::instance::request_adapter`, read at 30.0.1). What it
//! does not do is go on to the next adapter when the one it chose cannot make a device -
//! a driver that lists a GPU and then fails it - and then the window does not open at all.
//! So photon chooses (`WgpuSetupCreateNew::native_adapter_selector`; egui-wgpu does not let
//! wgpu's `force_fallback_adapter` be asked for): in wgpu's own order, each adapter tried
//! for a device in turn, the first that makes one taken. The device it makes is let go of
//! and made again by egui-wgpu - one more device made at launch, and only one when the
//! first adapter works.

use eframe::{egui_wgpu, wgpu};
use std::sync::Arc;

/// What is said once when the window is drawn without a graphics card.
pub const SOFTWARE: &str = "Drawn without a graphics card: photon will be slow.";

/// wgpu's own order of a kind of adapter (`get_order` in wgpu-core's `request_adapter`):
/// the GPUs first, by the power preference; an adapter that does not say what it is
/// (OpenGL's) after them; a virtual one; a software one last.
fn rank(kind: wgpu::DeviceType, power: wgpu::PowerPreference) -> u8 {
    use wgpu::DeviceType::*;
    let integrated_first = power == wgpu::PowerPreference::LowPower;
    match kind {
        DiscreteGpu if integrated_first => 2,
        IntegratedGpu if integrated_first => 1,
        DiscreteGpu => 1,
        IntegratedGpu => 2,
        Other => 3,
        VirtualGpu => 4,
        Cpu => 5,
    }
}

/// The adapters to try, by their place in the list wgpu gave: of each one's kind and
/// whether it can draw to the window's surface, those that can, in wgpu's order - a stable
/// sort, as wgpu's, so that of two of a kind the one listed first is tried first.
pub fn order(adapters: &[(wgpu::DeviceType, bool)], power: wgpu::PowerPreference) -> Vec<usize> {
    let mut usable: Vec<usize> = (0..adapters.len()).filter(|n| adapters[*n].1).collect();
    if power != wgpu::PowerPreference::None {
        usable.sort_by_key(|n| rank(adapters[*n].0, power));
    }
    usable
}

/// The wgpu setup that chooses by `order`, each adapter tried for a device.
pub fn setup() -> egui_wgpu::WgpuSetup {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    let (describe, power) = (Arc::clone(&setup.device_descriptor), setup.power_preference);
    setup.native_adapter_selector = Some(Arc::new(move |adapters, surface| {
        let kinds: Vec<(wgpu::DeviceType, bool)> = (adapters.iter())
            .map(|adapter| {
                let draws = surface
                    .is_none_or(|surface| !surface.get_capabilities(adapter).formats.is_empty());
                (adapter.get_info().device_type, draws)
            })
            .collect();
        let mut failed = Vec::new();
        for n in order(&kinds, power) {
            let adapter = &adapters[n];
            match pollster::block_on(adapter.request_device(&describe(adapter))) {
                Ok(_) => return Ok(adapter.clone()),
                Err(err) => {
                    let name = adapter.get_info().name;
                    tracing::warn!(%err, name, "a graphics adapter could not make a device");
                    failed.push(format!("{name}: {err}"));
                }
            }
        }
        Err(if failed.is_empty() {
            "no graphics adapter, not even a software one, can draw this window".to_owned()
        } else {
            format!("no graphics adapter could be used ({})", failed.join("; "))
        })
    }));
    setup.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::{DeviceType::*, PowerPreference};

    // A GPU first, a software adapter last, and one that cannot draw to the window not at
    // all - so a machine whose only drawing adapter is WARP or llvmpipe is drawn by it.
    #[test]
    fn a_software_adapter_is_tried_last_and_one_that_cannot_draw_never() {
        let adapters = [
            (Cpu, true),
            (DiscreteGpu, false),
            (IntegratedGpu, true),
            (Other, true),
        ];
        assert_eq!(
            order(&adapters, PowerPreference::HighPerformance),
            [2, 3, 0]
        );
        assert_eq!(order(&[(Cpu, true)], PowerPreference::HighPerformance), [0]);
        assert_eq!(
            order(&[(DiscreteGpu, false)], PowerPreference::HighPerformance),
            Vec::<usize>::new()
        );
    }

    // The power preference orders the GPUs as wgpu does, and none keeps the list's order.
    #[test]
    fn the_power_preference_orders_the_gpus() {
        let adapters = [
            (IntegratedGpu, true),
            (DiscreteGpu, true),
            (VirtualGpu, true),
        ];
        assert_eq!(
            order(&adapters, PowerPreference::HighPerformance),
            [1, 0, 2]
        );
        assert_eq!(order(&adapters, PowerPreference::LowPower), [0, 1, 2]);
        assert_eq!(order(&adapters, PowerPreference::None), [0, 1, 2]);
    }

    // Of two of a kind, the one wgpu listed first is tried first.
    #[test]
    fn two_of_a_kind_keep_their_order() {
        let adapters = [
            (Cpu, true),
            (DiscreteGpu, true),
            (Cpu, true),
            (DiscreteGpu, true),
        ];
        assert_eq!(
            order(&adapters, PowerPreference::HighPerformance),
            [1, 3, 0, 2]
        );
    }
}
