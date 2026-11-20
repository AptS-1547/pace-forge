//! 全部 `PACE_*` 环境开关的清单、默认值与当前取值。
//!
//! 排障时最常见的一句是「我设了环境变量，怎么好像没生效」。这里把开关
//! 集中成一张表：`pace-forge config` 打印默认值、当前值、以及它影响哪条
//! 判据；Web 启动时再打一行「非默认项」，日志里一眼能看见对方开了什么。

/// 一个开关的静态描述。
pub struct Entry {
    pub name: &'static str,
    pub default: &'static str,
    pub effect: &'static str,
    /// 用于「是否非默认」比较的实际值；`None` 表示只要设置就算非默认
    /// （口令、目录、自动推导这类没有可比较的字符串默认值）。
    pub default_value: Option<&'static str>,
}

/// 清单。新增 `env::var("PACE_*")` 时必须同步加到这里，否则 `config`
/// 会漏项——这条约定写在 CONTRIBUTING 里。
const ENTRIES: &[Entry] = &[
    Entry {
        name: "PACE_FORGE_TOKEN",
        default: "未设置",
        effect: "控制台口令。不设只接受本机访问；设了才允许同网段访问，并把 X-Pace-Token 当门槛",
        default_value: None,
    },
    Entry {
        name: "PACE_STRICT_POLICY",
        default: "0",
        effect: "1 = 策略拉取失败直接报错，不走「policy=0」兜底",
        default_value: Some("0"),
    },
    Entry {
        name: "PACE_FENCE_FULL",
        default: "0",
        effect: "1 = 围栏对象发完整结构（distance/id/name），默认只发 points",
        default_value: Some("0"),
    },
    Entry {
        name: "PACE_DUMP_POLICY",
        default: "未设置",
        effect: "目录路径。策略响应落到该目录，排查用；不设则完全不落盘",
        default_value: None,
    },
    Entry {
        name: "PACE_TZ_OFFSET_SEC",
        default: "28800（东八区）",
        effect: "OBS 时间戳的时区偏移（秒），跨时区部署用",
        default_value: Some("28800"),
    },
    Entry {
        name: "PACE_RUN_AREA_ID",
        default: "真实围栏 id（自动）",
        effect: "-1 时提交报文发 runAreaId=-1，退回旧行为做对照实验",
        default_value: None,
    },
    Entry {
        name: "PACE_PT_SCHEME",
        default: "自动（按有无必到点分流）",
        effect: "old|new。强制点位 type 口径，做对照实验用",
        default_value: None,
    },
    Entry {
        name: "PACE_BULGE_MAX",
        default: "60",
        effect: "几何规划时弧的鼓出上限（米，0~300）。设 0 更贴直线",
        default_value: Some("60"),
    },
    Entry {
        name: "PACE_RUN_MODE",
        default: "2",
        effect: "CLI run 的 runMode：1=计分跑，2=户外跑",
        default_value: Some("2"),
    },
    Entry {
        name: "PACE_STEP_M",
        default: "5.0",
        effect: "CLI 的几何采样间距（米），只管形状不管上报节奏",
        default_value: Some("5.0"),
    },
];

/// 当前取值；未设置或空串返回 `None`。
pub fn current(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// 打印整张表（`pace-forge config`）。
pub fn print_table() {
    println!("PACE_* 开关（默认值 → 当前值 → 影响）\n");
    for e in ENTRIES {
        let val = current(e.name).unwrap_or_else(|| e.default.to_string());
        let shown = if e.name.contains("TOKEN") && current(e.name).is_some() {
            // 口令不打印原文：终端截图、issue 贴日志都是常态。
            "已设置（不显示值）".to_string()
        } else {
            val
        };
        let changed = match current(e.name) {
            Some(v) if e.default_value.is_some_and(|d| d != v) => "  ← 非默认",
            Some(_) if e.default_value.is_none() => "  ← 非默认",
            _ => "",
        };
        println!("{} = {}{}", e.name, shown, changed);
        println!("    默认 {}：{}", e.default, e.effect);
    }
    println!("\n以上为全部开关。没有列出来的 PACE_* 都是历史残留或无效拼写。");
}

/// Web / CLI 启动时打一行「非默认项」。全默认返回 `None`。
pub fn non_default_line() -> Option<String> {
    let mut items: Vec<String> = Vec::new();
    for e in ENTRIES {
        if let Some(v) = current(e.name) {
            if e.name.contains("TOKEN") {
                items.push(format!("{}=（已设置）", e.name));
            } else if e.default_value.is_none() || e.default_value != Some(v.as_str()) {
                items.push(format!("{}={}", e.name, v));
            }
        }
    }
    if items.is_empty() {
        None
    } else {
        Some(items.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 清单里不能有重名——重名会让启动那行「非默认项」打印两次。
    #[test]
    fn entries_are_unique() {
        let mut names: Vec<&str> = ENTRIES.iter().map(|e| e.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ENTRIES.len(), "开关名重复");
    }

    /// 每个名字都以 PACE_ 开头，避免混进 HOME / USER 这类系统变量。
    #[test]
    fn entries_are_pace_prefixed() {
        for e in ENTRIES {
            assert!(e.name.starts_with("PACE_"), "{} 不是 PACE_ 前缀", e.name);
        }
    }
}
