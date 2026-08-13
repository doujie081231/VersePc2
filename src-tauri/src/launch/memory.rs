// launch/memory.rs — JVM 内存决策纯函数
// 1:1 翻译自 server/launch/memory-resolver.js + memory-mode-resolver.js + memory-optimize-resolver.js

/// 旧版默认内存值（用于判断用户是否改过全局设置）
pub const DEFAULT_LEGACY_MAX_MEMORY: u64 = 4096;

/// 内存模式
#[derive(Clone, Debug, PartialEq)]
pub enum MemoryMode {
    Auto,
    Custom(u64),
}

/// 决策内存模式（对应 memory-mode-resolver.js）
/// 优先级 1: 前端 launch_settings 显式选择
/// 优先级 2: settings.maxMemory ≠ 4096 → custom
/// 优先级 3: 默认 auto
pub fn resolve_memory_mode(
    settings_max_memory: u64,
    has_launch_settings: bool,
    launch_memory_mode: Option<&str>,
    launch_memory_value: Option<u64>,
) -> MemoryMode {
    if has_launch_settings {
        let mode = launch_memory_mode.unwrap_or("auto");
        if mode == "custom" {
            return MemoryMode::Custom(launch_memory_value.unwrap_or(4096));
        }
        return MemoryMode::Auto;
    }
    if settings_max_memory != DEFAULT_LEGACY_MAX_MEMORY {
        return MemoryMode::Custom(settings_max_memory);
    }
    MemoryMode::Auto
}

/// 解析 JVM 最大内存（对应 memory-resolver.js）
/// 自动模式：物理内存 1/4 基础值 + Mod 分级加成
/// custom 模式：直接返回用户指定值
pub fn resolve_max_memory(
    mode: &MemoryMode,
    total_mb: u64,
    free_mb: u64,
    mod_count: u64,
) -> u64 {
    match mode {
        MemoryMode::Custom(v) => *v,
        MemoryMode::Auto => {
            const GB_TO_MB: u64 = 1024;
            const ALIGN_MB: u64 = 128;
            const MIN_BASE_MB: u64 = 256;

            // 基础值：物理内存 1/4，对齐 128MB，保底 256MB
            let base_mb = ((total_mb / 4) / ALIGN_MB) * ALIGN_MB;
            let base_mb = base_mb.max(MIN_BASE_MB);

            // Mod 加成 4 级目标（GB）
            let ram_min_gb = 0.5 + (mod_count as f64) / 150.0;
            let ram_target1_gb = 1.5 + (mod_count as f64) / 90.0;
            let ram_target2_gb = 2.7 + (mod_count as f64) / 50.0;
            let ram_target3_gb = 4.5 + (mod_count as f64) / 25.0;

            let mut ram_give_mb: f64 = 0.0;
            let mut ram_available_gb = (total_mb as f64) / (GB_TO_MB as f64);

            let stages: [(f64, f64); 4] = [
                (ram_target1_gb, 1.0),
                (ram_target2_gb - ram_target1_gb, 0.7),
                (ram_target3_gb - ram_target2_gb, 0.4),
                (ram_target3_gb, 0.15),
            ];

            for (delta_gb, ratio) in stages {
                ram_give_mb += (ram_available_gb * ratio).min(delta_gb) * (GB_TO_MB as f64);
                ram_available_gb -= delta_gb / ratio;
                if ram_available_gb < 0.1 {
                    break;
                }
            }

            let min_required_mb = (ram_min_gb * (GB_TO_MB as f64)).floor();
            ram_give_mb = ram_give_mb.max(min_required_mb);

            let mut auto_mb = (base_mb as f64).max(ram_give_mb.floor());

            // 物理内存总量约束（分档保留系统占用）
            let system_reserve = if total_mb <= 4096 {
                1536
            } else if total_mb <= 8192 {
                2048
            } else {
                2560
            };
            let physical_cap = (total_mb.saturating_sub(system_reserve)).max(512);
            auto_mb = auto_mb.min(physical_cap as f64);

            // 当前可用内存约束（保留 15% 给 JVM 自身和系统波动）
            let free_cap = ((free_mb as f64) * 0.85).floor();
            if auto_mb > free_cap {
                auto_mb = free_cap.max(min_required_mb);
            }

            // 封顶 16GB，对齐 256MB
            let mut result = auto_mb.max(512.0).min(16384.0) as u64;
            result = (result / 256) * 256;
            result
        }
    }
}

/// 决策是否执行启动前内存优化（对应 memory-optimize-resolver.js）
/// 优先级 1: 版本独立设置 on/off
/// 优先级 2: 全局设置必须显式开启
/// 优先级 3: 大内存机器（≥12GB）跳过
/// 优先级 4: 大型整合包（mod ≥ 100）跳过
/// 优先级 5: 可用内存充足（> 4GB）跳过
pub fn should_run_memory_optimize(
    auto_memory_optimize: bool,
    version_mem_optimize: &str,
    total_mem_mb: u64,
    mod_count: u64,
    free_mb: u64,
) -> bool {
    if version_mem_optimize == "on" {
        return true;
    }
    if version_mem_optimize == "off" {
        return false;
    }
    if !auto_memory_optimize {
        return false;
    }
    if total_mem_mb >= 12288 {
        return false;
    }
    if mod_count >= 100 {
        return false;
    }
    if free_mb > 4096 {
        return false;
    }
    true
}

/// 计算最小内存（对应 args-builder.js 中的逻辑）
/// maxMemMB / 2，对齐 256MB，最小 512MB，最大不超过 maxMemMB
pub fn resolve_min_memory(max_mem_mb: u64) -> u64 {
    let mut min = (max_mem_mb / 2) / 256 * 256;
    min = min.max(512);
    min.min(max_mem_mb)
}
