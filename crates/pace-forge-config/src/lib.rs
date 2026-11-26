//! pace-forge 的类型化配置：`config.toml` + `PACE_*` 环境变量覆盖层。
//!
//! 分层与语义：
//!
//! ```text
//! 环境变量（PACE_*）  >  config.toml  >  内置默认值
//! ```
//!
//! 启动时一次性加载并校验（fail-fast：非法值、未知键、缺一半的围栏锚点
//! 都直接报错退出，不再静默回落默认值），进程内只读快照（[`store`] +
//! [`global`]）。运行期可变的设置（比如网页里保存的出口代理）不属于这里，
//! 仍归各自模块的 runtime 槽位管。
//!
//! 所有开关的键名、环境变量名、取值词汇表、默认值、影响说明都集中在
//! [`TABLE`] 一张注册表里：TOML 解析、env 解析、`pace-forge config` 打表
//! 用的是同一份定义，不存在「文档表」和「实际读取」两套东西对不上的问题。
//!
//! 与旧实现（`src/config.rs` 手写 ENTRIES 清单 + 各处散读 `env::var`）的
//! 兼容关系：
//! - 全部 `PACE_*` 变量名保留，语义不变（个别例外见条目注释）；
//! - 新增 `config.toml` 一层，环境变量覆盖它；
//! - 非法值从「静默回落默认」变为「启动报错」——这是本次重构的目的。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

pub const CONFIG_FILE_NAME: &str = "config.toml";

// ---------------------------------------------------------------------------
// 值与来源
// ---------------------------------------------------------------------------

/// 解析后的单键值。键的 kind 由注册表保证，取值时按 kind 走对应入口。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Value {
    fn as_str(&self) -> &str {
        match self {
            Value::Str(s) => s,
            other => panic!("配置键的 kind 不是字符串，却是 {other:?}"),
        }
    }
    fn as_int(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            other => panic!("配置键的 kind 不是整数，却是 {other:?}"),
        }
    }
    fn as_float(&self) -> f64 {
        match self {
            Value::Float(v) => *v,
            other => panic!("配置键的 kind 不是浮点数，却是 {other:?}"),
        }
    }
    fn as_bool(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            other => panic!("配置键的 kind 不是布尔，却是 {other:?}"),
        }
    }
}

/// 一个键的取值来自哪一层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Default,
    Toml,
    Env,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Default => "默认",
            Source::Toml => "config.toml",
            Source::Env => "env",
        }
    }
}

/// 键 → 来源。随 [`Config`] 一起加载，供打表和排障显示。
pub type Sources = HashMap<&'static str, Source>;

// ---------------------------------------------------------------------------
// 类型化配置（消费方读这里）
// ---------------------------------------------------------------------------

/// Web 驻留进程的身份与落盘位置。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Server {
    /// 控制台口令；空 = 只接受本机访问。
    pub token: String,
    /// 数据目录覆盖；空 = `$HOME/.pace-forge`。
    pub data_dir: String,
}

/// 出口与时间口径。
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    /// 提交链路出口 `http://host:port`；空 = 直连。网页保存的设置优先于本值。
    pub egress_proxy: String,
    /// 追加信任根的 PEM 文件路径；空 = 不追加。
    pub extra_ca: String,
    /// OBS 时间戳的时区偏移（秒），默认东八区。
    pub tz_offset_sec: i64,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            egress_proxy: String::new(),
            extra_ca: String::new(),
            tz_offset_sec: 8 * 3600,
        }
    }
}

/// 上报的设备口径。
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// 上报的 appVersion，默认取真机版本（7.4.20 / code 411）。
    pub app_version: String,
    /// 机型 code 或平台代号；空 = cpuModel 发 ABI。
    pub model: String,
    /// 五个硬件头（androidId/blMac/wifiMac/IMEI/CustomDeviceId）发派生假值还是空串。
    pub hw_headers: HwHeaders,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            app_version: DEFAULT_APP_VERSION.into(),
            model: String::new(),
            hw_headers: HwHeaders::Empty,
        }
    }
}

/// 真机版本（7.4.20 / code 411，2026-11 从设备上取）。空串配置回落到这里。
pub const DEFAULT_APP_VERSION: &str = "7.4.20";

/// 五个硬件头的口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HwHeaders {
    /// 发空串（7.4.21 硬编码口径，默认）。
    #[default]
    Empty,
    /// 发派生假值（对照实验用）。
    Derived,
}

/// 刷跑与提交的行为开关。
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// CLI 刷跑的 runMode：1=计分跑，2=户外跑。
    pub run_mode: i64,
    /// 几何采样间距（米）。
    pub step_m: f64,
    /// 几何规划时弧的鼓出上限（米）。
    pub bulge_max: f64,
    /// 点位 type 口径。
    pub pt_scheme: PtScheme,
    /// 固定点（isFixed/forceNormal）口径。
    pub fixed_point: FixedPoint,
    /// 必到点只认 10 米内，不自动放开半径。
    pub fixed_strict: bool,
    /// 策略拉取失败直接报错，不走 runMode=2 兜底。
    pub strict_policy: bool,
    /// 围栏对象发完整结构（distance/id/name），默认只发 points。
    pub fence_full: bool,
    /// Some(-1) = 提交报文发 runAreaId=-1（旧行为对照）。
    pub run_area_id: Option<i64>,
    /// 显式围栏锚点（成对出现，缺一半在加载时报错）。
    pub center_lat: Option<f64>,
    /// 同上。
    pub center_lng: Option<f64>,
    /// CLI 默认试操场轨迹（web 不读这个，走自己的 mode）。
    pub prefer_stadium: bool,
    /// 超日上限也继续提交。
    pub ignore_daily_limit: bool,
    /// 策略响应落盘目录（排障用）；空 = 不落盘。
    pub dump_policy: String,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            run_mode: 2,
            step_m: 5.0,
            bulge_max: 60.0,
            pt_scheme: PtScheme::Auto,
            fixed_point: FixedPoint::Auto,
            fixed_strict: false,
            strict_policy: false,
            fence_full: false,
            run_area_id: None,
            center_lat: None,
            center_lng: None,
            prefer_stadium: false,
            ignore_daily_limit: false,
            dump_policy: String::new(),
        }
    }
}

/// 点位 type 口径（`PACE_PT_SCHEME` / `run.pt_scheme`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PtScheme {
    /// 按有无必到点自动分流（默认）。
    #[default]
    Auto,
    /// 老样本口径（显式 `old` / `marks`）。
    Old,
    /// 新设备口径（显式 `new`）。
    New,
}

/// 固定点口径（`PACE_FIXED_POINT` / `run.fixed_point`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FixedPoint {
    /// 自动（默认）。
    #[default]
    Auto,
    /// 全部发 0（`all0` / `allnormal`；旧变量 `=1`）。
    AllNormal,
    /// 跟随服务端（`server` / `follow`；旧变量 `=0`）。
    FollowServer,
}

/// 加载完成的配置。消费方按 `cfg.run.bulge_max` 这样的路径读字段，
/// 「值来自哪一层」看 [`Config::sources`]。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub server: Server,
    pub network: Network,
    pub device: Device,
    pub run: Run,
    pub sources: Sources,
}

impl Config {
    /// 全部键的当前显示值（打表和排障用；口令类值在这里就打码）。
    ///
    /// 这份映射是注册表与结构体字段之间的**唯一桥梁**：新增开关时两边
    /// 各改一处，`registry_matches_config_keys` 测试保证不改就编译失败/测试失败。
    pub fn key_values(&self) -> Vec<(&'static str, String)> {
        let f = |v: f64| fmt_float(v);
        let b = |v: bool| if v { "1" } else { "0" };
        let opt = |v: Option<f64>| v.map(f).unwrap_or_else(|| "未设置".into());
        vec![
            (
                "server.token",
                if self.server.token.is_empty() {
                    "未设置".into()
                } else {
                    "已设置（不显示值）".into()
                },
            ),
            (
                "server.data_dir",
                if self.server.data_dir.is_empty() {
                    "未设置（$HOME/.pace-forge）".into()
                } else {
                    self.server.data_dir.clone()
                },
            ),
            (
                "network.egress_proxy",
                if self.network.egress_proxy.is_empty() {
                    "未设置（直连）".into()
                } else {
                    self.network.egress_proxy.clone()
                },
            ),
            (
                "network.extra_ca",
                if self.network.extra_ca.is_empty() {
                    "未设置".into()
                } else {
                    self.network.extra_ca.clone()
                },
            ),
            (
                "network.tz_offset_sec",
                self.network.tz_offset_sec.to_string(),
            ),
            ("device.app_version", self.device.app_version.clone()),
            (
                "device.model",
                if self.device.model.is_empty() {
                    "未设置（cpuModel 发 ABI）".into()
                } else {
                    self.device.model.clone()
                },
            ),
            (
                "device.hw_headers",
                match self.device.hw_headers {
                    HwHeaders::Empty => "empty".into(),
                    HwHeaders::Derived => "derived".into(),
                },
            ),
            ("run.run_mode", self.run.run_mode.to_string()),
            ("run.step_m", f(self.run.step_m)),
            ("run.bulge_max", f(self.run.bulge_max)),
            (
                "run.pt_scheme",
                match self.run.pt_scheme {
                    PtScheme::Auto => "auto",
                    PtScheme::Old => "old",
                    PtScheme::New => "new",
                }
                .into(),
            ),
            (
                "run.fixed_point",
                match self.run.fixed_point {
                    FixedPoint::Auto => "auto",
                    FixedPoint::AllNormal => "allnormal",
                    FixedPoint::FollowServer => "server",
                }
                .into(),
            ),
            ("run.fixed_strict", b(self.run.fixed_strict).into()),
            ("run.strict_policy", b(self.run.strict_policy).into()),
            ("run.fence_full", b(self.run.fence_full).into()),
            (
                "run.run_area_id",
                match self.run.run_area_id {
                    Some(-1) => "-1（旧行为）".into(),
                    Some(v) => v.to_string(),
                    None => "未设置（发真实围栏 id）".into(),
                },
            ),
            ("run.center_lat", opt(self.run.center_lat)),
            ("run.center_lng", opt(self.run.center_lng)),
            ("run.prefer_stadium", b(self.run.prefer_stadium).into()),
            (
                "run.ignore_daily_limit",
                b(self.run.ignore_daily_limit).into(),
            ),
            (
                "run.dump_policy",
                if self.run.dump_policy.is_empty() {
                    "未设置（不落盘）".into()
                } else {
                    self.run.dump_policy.clone()
                },
            ),
        ]
    }
}

fn fmt_float(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        v.to_string()
    }
}

// ---------------------------------------------------------------------------
// 解析器（每个键一个，全部来源共用同一份）
// ---------------------------------------------------------------------------

/// 保留原文的字符串键：口令、目录类。旧实现里这几个值不 trim
/// （`PACE_FORGE_TOKEN` 带尾随空格也是合法口令），必须照搬。
fn parse_str_raw(raw: &str) -> Result<Value, String> {
    Ok(Value::Str(raw.to_string()))
}

/// 去空白字符串键：版本号、代理、机型。旧实现就是 `trim` 之后用。
fn parse_str_trim(raw: &str) -> Result<Value, String> {
    Ok(Value::Str(raw.trim().to_string()))
}

/// 布尔词汇表统一为 0/1、true/false、yes/no（大小写不敏感）。
///
/// 旧的散装解析各键口径不一（fence_full 只认 "1"、fixed_strict 还认
/// "TRUE"、prefer_stadium 认 yes），文档里写过的值全部行为不变；这次
/// 统一词汇表属于 issue #53 里声明的兼容性放宽，其余词汇外的值从「静默
/// 回落」改为「启动报错」。
fn parse_bool(raw: &str) -> Result<Value, String> {
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Ok(Value::Bool(true)),
        "0" | "false" | "no" => Ok(Value::Bool(false)),
        other => Err(format!("要 0/1、true/false 或 yes/no，收到 {other:?}")),
    }
}

fn int_in_range(min: i64, max: i64) -> impl Fn(&str) -> Result<Value, String> {
    move |raw: &str| {
        // 旧实现对原文精确 parse（" 60" 会回落默认），现在词汇外一律报错
        let v: i64 = raw
            .parse()
            .map_err(|_| format!("要 {min}~{max} 的整数，收到 {raw:?}"))?;
        if (min..=max).contains(&v) {
            Ok(Value::Int(v))
        } else {
            Err(format!("要 {min}~{max} 的整数，收到 {v}"))
        }
    }
}

/// 任意 i64（run_mode：服务端 0/2~6 都当自由跑，实验值不设上限）。
fn parse_int_any(raw: &str) -> Result<Value, String> {
    raw.parse::<i64>()
        .map(Value::Int)
        .map_err(|_| format!("要整数，收到 {raw:?}"))
}

fn float_in_range(min: f64, max: f64) -> impl Fn(&str) -> Result<Value, String> {
    move |raw: &str| {
        let v: f64 = raw
            .parse()
            .map_err(|_| format!("要 {min}~{max} 的数字，收到 {raw:?}"))?;
        if v.is_finite() && (min..=max).contains(&v) {
            Ok(Value::Float(v))
        } else {
            Err(format!("要 {min}~{max} 范围内的有限数字，收到 {raw:?}"))
        }
    }
}

fn choice(list: &'static [&'static str]) -> impl Fn(&str) -> Result<Value, String> {
    move |raw: &str| {
        if list.contains(&raw) {
            Ok(Value::Str(raw.to_string()))
        } else {
            Err(format!("要 {} 之一，收到 {raw:?}", list.join(" / ")))
        }
    }
}

/// 硬件头：`empty|derived`，env 老用法 `1/true`（=derived）、`0/false`（=empty）
/// 也认；大小写不敏感是这次统一的一部分。
fn parse_hw_headers(raw: &str) -> Result<Value, String> {
    match raw.to_ascii_lowercase().as_str() {
        "empty" | "0" | "false" => Ok(Value::Str("empty".into())),
        "derived" | "1" | "true" => Ok(Value::Str("derived".into())),
        other => Err(format!("要 empty 或 derived，收到 {other:?}")),
    }
}

/// 点位口径：`auto|old|marks|new`，marks 归一成 old。原文精确匹配。
fn parse_pt_scheme(raw: &str) -> Result<Value, String> {
    match raw {
        "auto" => Ok(Value::Str("auto".into())),
        "old" | "marks" => Ok(Value::Str("old".into())),
        "new" => Ok(Value::Str("new".into())),
        other => Err(format!("要 auto / old / new 之一，收到 {other:?}")),
    }
}

/// 固定点口径：`auto|all0|allnormal|server|follow`。原文精确匹配。
fn parse_fixed_point(raw: &str) -> Result<Value, String> {
    match raw {
        "auto" => Ok(Value::Str("auto".into())),
        "all0" | "allnormal" => Ok(Value::Str("allnormal".into())),
        "server" | "follow" => Ok(Value::Str("server".into())),
        other => Err(format!(
            "要 auto / all0 / allnormal / server / follow 之一，收到 {other:?}"
        )),
    }
}

/// 旧变量 `PACE_FORCE_NORMAL_POINT` 的布尔词汇：0→server，1→allnormal。
fn parse_fixed_point_legacy(raw: &str) -> Result<Value, String> {
    match raw {
        "0" | "false" | "no" => Ok(Value::Str("server".into())),
        "1" | "true" | "yes" => Ok(Value::Str("allnormal".into())),
        other => Err(format!("要 0 或 1（旧变量口径），收到 {other:?}")),
    }
}

// 带范围/带词汇表的键：注册表要的是函数指针，这里给每个键一个具名入口。
fn parse_tz_offset(raw: &str) -> Result<Value, String> {
    int_in_range(-72_000, 72_000)(raw)
}
fn parse_run_mode(raw: &str) -> Result<Value, String> {
    parse_int_any(raw)
}
fn parse_step_m(raw: &str) -> Result<Value, String> {
    // 旧实现接受任意 f64（连 0 和负数都收，然后生成坏轨迹）；正数全放开，
    // 只挡 0/负数/inf 这类明显是打错的值。
    float_in_range(0.001, 100_000.0)(raw)
}
fn parse_bulge_max(raw: &str) -> Result<Value, String> {
    float_in_range(0.0, 300.0)(raw)
}
fn parse_center_lat(raw: &str) -> Result<Value, String> {
    float_in_range(-90.0, 90.0)(raw)
}
fn parse_center_lng(raw: &str) -> Result<Value, String> {
    float_in_range(-180.0, 180.0)(raw)
}
fn parse_run_area(raw: &str) -> Result<Value, String> {
    choice(&["-1", "minus1"])(raw)
}

// ---------------------------------------------------------------------------
// 注册表（唯一事实源）
// ---------------------------------------------------------------------------

/// 单键解析器：`原文 → 类型化值`，TOML 和 env 两个来源共用同一份。
type RawParser = fn(&str) -> Result<Value, String>;

/// 一个开关的完整定义：键名、env 名、解析器、默认显示、影响说明。
struct EntryDef {
    /// TOML 键路径，如 `run.bulge_max`。
    key: &'static str,
    /// 环境变量名（全部键都有）。
    env: &'static str,
    /// 旧环境变量名：仅当主变量未设时读，词汇表单独解析。
    env_legacy: Option<&'static str>,
    parse: RawParser,
    parse_legacy: Option<RawParser>,
    /// 打表时显示的默认值（描述性文本）。
    default_display: &'static str,
    effect: &'static str,
}

macro_rules! entry {
    ($key:literal, $env:literal, $parse:ident, $default:literal, $effect:literal) => {
        EntryDef {
            key: $key,
            env: $env,
            env_legacy: None,
            parse: $parse,
            parse_legacy: None,
            default_display: $default,
            effect: $effect,
        }
    };
}

/// 清单。新增开关 = 这里加一条 + [`Config`] 加字段 + [`Config::key_values`]
/// 加一行显示——三处少一处，`registry_matches_config_keys` 测试直接红。
/// 口令类的打码在 [`Config::key_values`] 的对应分支里做（原值不进显示）。
const TABLE: &[EntryDef] = &[
    entry!(
        "server.token",
        "PACE_FORGE_TOKEN",
        parse_str_raw,
        "未设置",
        "控制台口令。不设只接受本机访问；设了才允许同网段访问，并把 X-Pace-Token 当门槛"
    ),
    entry!(
        "server.data_dir",
        "PACE_FORGE_DATA_DIR",
        parse_str_raw,
        "未设置（$HOME/.pace-forge）",
        "账号库、session、缓存的落盘根目录；设了就不再追加 .pace-forge。设错就会「账号全没了」。不影响 config.toml 的搜索（~ 始终用系统主目录）"
    ),
    entry!(
        "network.egress_proxy",
        "PACE_EGRESS_PROXY",
        parse_str_trim,
        "未设置（直连）",
        "http://host:port。提交链路走这个出口；优先级：网页保存的 > env > config.toml。格式合法性在启用时校验，非法只报一次并按直连走"
    ),
    entry!(
        "network.extra_ca",
        "PACE_EXTRA_CA",
        parse_str_raw,
        "未设置",
        "PEM 文件路径。把里面的证书加到公共 CA 根之上（沙箱/企业网关自签 CA）；文件读不到时按默认 CA 走并报一次"
    ),
    entry!(
        "network.tz_offset_sec",
        "PACE_TZ_OFFSET_SEC",
        parse_tz_offset,
        "28800（东八区）",
        "OBS 时间戳的时区偏移（秒），跨时区部署用；±20 小时以内"
    ),
    entry!(
        "device.app_version",
        "PACE_APP_VERSION",
        parse_str_trim,
        "7.4.20",
        "上报的 appVersion，默认取真机版本（7.4.20 / code 411）。改它等于自报用别的客户端版本"
    ),
    entry!(
        "device.model",
        "PACE_DEVICE_MODEL",
        parse_str_trim,
        "未设置（cpuModel 发 ABI）",
        "机型 code 或平台代号。命中 data/devices.json 时 cpuModel 发 SoC 型号（SM8550），未收录仍发 ABI 并提示"
    ),
    entry!(
        "device.hw_headers",
        "PACE_DEVICE_HW_HEADERS",
        parse_hw_headers,
        "empty（发空串）",
        "五个硬件头（androidId/blMac/wifiMac/IMEI/CustomDeviceId）：derived 发派生假值；默认空串，与 7.4.21 硬编码一致。DeviceId 不受影响"
    ),
    entry!(
        "run.run_mode",
        "PACE_RUN_MODE",
        parse_run_mode,
        "2",
        "CLI 刷跑的 runMode：1=计分跑，2=户外跑；任意整数都收（0/2~6 服务端都当自由跑，实验值不设限）"
    ),
    entry!(
        "run.step_m",
        "PACE_STEP_M",
        parse_step_m,
        "5.0",
        "CLI 的几何采样间距（米，正数），只管形状不管上报节奏；0/负数/inf 这类打错的值启动时报错"
    ),
    entry!(
        "run.bulge_max",
        "PACE_BULGE_MAX",
        parse_bulge_max,
        "60",
        "几何规划时弧的鼓出上限（米）。设 0 更贴直线"
    ),
    entry!(
        "run.pt_scheme",
        "PACE_PT_SCHEME",
        parse_pt_scheme,
        "自动（按有无必到点分流）",
        "old / new。强制点位 type 口径，做对照实验用"
    ),
    EntryDef {
        key: "run.fixed_point",
        env: "PACE_FIXED_POINT",
        env_legacy: Some("PACE_FORCE_NORMAL_POINT"),
        parse: parse_fixed_point,
        parse_legacy: Some(parse_fixed_point_legacy),
        default_display: "auto",
        effect: "固定点口径：all0/allnormal=全部发 0，server/follow=跟随服务端。旧变量 PACE_FORCE_NORMAL_POINT 仍认（0→server，1→all0），仅在主变量未设时生效",
    },
    entry!(
        "run.fixed_strict",
        "PACE_FIXED_STRICT",
        parse_bool,
        "0",
        "1 = 必到点只认 10 米内，够不着就报缺失，不自动放开半径"
    ),
    entry!(
        "run.strict_policy",
        "PACE_STRICT_POLICY",
        parse_bool,
        "0",
        "1 = 策略拉取失败直接报错，不走「policy=0」兜底"
    ),
    entry!(
        "run.fence_full",
        "PACE_FENCE_FULL",
        parse_bool,
        "0",
        "1 = 围栏对象发完整结构（distance/id/name），默认只发 points"
    ),
    entry!(
        "run.run_area_id",
        "PACE_RUN_AREA_ID",
        parse_run_area,
        "未设置（发真实围栏 id）",
        "-1 = 提交报文发 runAreaId=-1，退回旧行为做对照实验"
    ),
    entry!(
        "run.center_lat",
        "PACE_CENTER_LAT",
        parse_center_lat,
        "未设置",
        "围栏锚点纬度。与 center_lng 必须成对出现，缺一半启动时报错（旧行为是静默忽略）"
    ),
    entry!(
        "run.center_lng",
        "PACE_CENTER_LNG",
        parse_center_lng,
        "未设置",
        "围栏锚点经度。与 center_lat 必须成对出现"
    ),
    entry!(
        "run.prefer_stadium",
        "PACE_PREFER_STADIUM",
        parse_bool,
        "0",
        "1 = CLI 默认试操场轨迹（issue #37）。web 不读这个，走自己的跑法选择"
    ),
    entry!(
        "run.ignore_daily_limit",
        "PACE_IGNORE_DAILY_LIMIT",
        parse_bool,
        "0",
        "1 = 超日上限也继续提交。旧行为是「设了就算」（连 =0 都生效），现在是标准布尔：0 就是关"
    ),
    entry!(
        "run.dump_policy",
        "PACE_DUMP_POLICY",
        parse_str_raw,
        "未设置（不落盘）",
        "目录路径。策略响应落到该目录排查用；策略响应里有考生身份字段，默认不落盘"
    ),
];

fn find_entry(key: &str) -> Option<&'static EntryDef> {
    TABLE.iter().find(|e| e.key == key)
}

// ---------------------------------------------------------------------------
// 加载：定位文件 → TOML 层 → env 覆盖层 → 交叉校验 → 结构体
// ---------------------------------------------------------------------------

/// 加载配置。`explicit` 来自 `--config <路径>` 或 `PACE_CONFIG`。
///
/// 没有找到配置文件时返回全默认（这是正常情况，不是错误）。
pub fn load(explicit: Option<&str>) -> Result<Config, String> {
    load_from(explicit, &real_env)
}

fn real_env(name: &str) -> Option<String> {
    // 空串过滤在 load_from 的 env 通道里统一做；这里只做 UTF-8 检查
    std::env::var(name).ok()
}

pub(crate) fn load_from(
    explicit: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Config, String> {
    let path = match explicit.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let pb = PathBuf::from(p);
            if !pb.is_file() {
                return Err(format!("--config 指定的文件不存在: {}", pb.display()));
            }
            Some(pb)
        }
        None => discover(env),
    };
    let mut errs: Vec<String> = Vec::new();
    let mut values: HashMap<&'static str, Value> = HashMap::new();
    let mut sources: Sources = HashMap::new();

    // ① TOML 层：文件存在才解析；整份文件全部解析、全部报错，
    //    不因为第一个坏键停下。没有文件不是错误——env 层照常生效。
    if let Some(path) = path {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("读配置文件 {} 失败: {e}", path.display()))?;
        let doc: toml::Value =
            toml::from_str(&raw).map_err(|e| format!("{} 不是合法 TOML: {e}", path.display()))?;
        let mut flat: Vec<(String, String)> = Vec::new();
        flatten(&doc, String::new(), &path, &mut flat, &mut errs);
        for (key, raw) in flat {
            let Some(entry) = find_entry(&key) else {
                errs.push(format!(
                    "{}: 未知键 {key:?}。可用键见 config.example.toml 或 `pace-forge config`",
                    path.display()
                ));
                continue;
            };
            match (entry.parse)(&raw) {
                Ok(v) => {
                    values.insert(entry.key, v);
                    sources.insert(entry.key, Source::Toml);
                }
                Err(msg) => errs.push(format!(
                    "{}: {} = {raw:?} 无效: {msg}",
                    path.display(),
                    entry.key
                )),
            }
        }
    }

    // ② env 层：主变量优先，未设才看旧变量
    for entry in TABLE {
        // 空串 = 未设（与旧 `filter(|v| !v.is_empty())` 一致）；过滤在 env
        // 通道里做，保证任何 env 来源都有这条语义
        let hit = env(entry.env)
            .filter(|raw| !raw.is_empty())
            .map(|raw| (entry.env, raw, entry.parse))
            .or_else(|| {
                entry.env_legacy.and_then(|name| {
                    env(name)
                        .filter(|raw| !raw.is_empty())
                        .map(|raw| (name, raw, entry.parse_legacy.unwrap_or(entry.parse)))
                })
            });
        let Some((name, raw, parse)) = hit else {
            continue;
        };
        match parse(&raw) {
            Ok(v) => {
                values.insert(entry.key, v);
                sources.insert(entry.key, Source::Env);
            }
            Err(msg) => errs.push(format!("环境变量 {name}={raw:?} 无效: {msg}")),
        }
    }

    // ③ 交叉校验：围栏锚点必须成对
    let has_lat = values.contains_key("run.center_lat");
    let has_lng = values.contains_key("run.center_lng");
    if has_lat != has_lng {
        errs.push(
            "run.center_lat / run.center_lng（PACE_CENTER_LAT / PACE_CENTER_LNG）必须成对出现：只设一个会被静默忽略，这类坑现在直接报错".into(),
        );
    }

    if !errs.is_empty() {
        return Err(errs.join("\n  - ").trim_start_matches("\n  - ").to_string());
    }

    let mut cfg = Config::from_values(&values);
    cfg.sources = sources;
    Ok(cfg)
}

/// `--config` / `PACE_CONFIG` 未指定时的搜索顺序：`./config.toml` →
/// `~/.pace-forge/config.toml`。都不存在 = 无配置文件，全默认。
fn discover(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let cwd = PathBuf::from(CONFIG_FILE_NAME);
    if cwd.is_file() {
        return Some(cwd);
    }
    let home = home_dir(env)?;
    let p = home.join(".pace-forge").join(CONFIG_FILE_NAME);
    p.is_file().then_some(p)
}

fn home_dir(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    env("HOME")
        .or_else(|| env("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// 把 TOML 文档拍平成 `键路径 → 字符串`，让 TOML 值和 env 值走同一个解析器。
/// 数组、_datetime、嵌套表下的意外形态都算错误。
fn flatten(
    node: &toml::Value,
    prefix: String,
    path: &Path,
    out: &mut Vec<(String, String)>,
    errs: &mut Vec<String>,
) {
    match node {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(v, key, path, out, errs);
            }
        }
        toml::Value::String(s) => out.push((prefix, s.clone())),
        toml::Value::Integer(i) => out.push((prefix, i.to_string())),
        toml::Value::Float(f) => out.push((prefix, f.to_string())),
        toml::Value::Boolean(b) => out.push((prefix, b.to_string())),
        other => errs.push(format!(
            "{}: {prefix} 的值类型不支持（{}），字符串/数字/布尔都可以",
            path.display(),
            type_name(other)
        )),
    }
}

fn type_name(v: &toml::Value) -> &'static str {
    match v {
        toml::Value::String(_) => "字符串",
        toml::Value::Integer(_) => "整数",
        toml::Value::Float(_) => "浮点数",
        toml::Value::Boolean(_) => "布尔",
        toml::Value::Datetime(_) => "日期时间",
        toml::Value::Array(_) => "数组",
        toml::Value::Table(_) => "表",
    }
}

impl Config {
    /// 注册表值 → 结构体字段。键来自 [`TABLE`]，这里必须全覆盖；
    /// 漏一个键，该开关就永远到不了字段上——`registry_matches_config_keys`
    /// 测试保证键集合一致，本函数的 `_` 分支兜住理论上的不一致。
    fn from_values(values: &HashMap<&'static str, Value>) -> Config {
        let mut cfg = Config::default();
        for (key, v) in values {
            match *key {
                "server.token" => cfg.server.token = v.as_str().to_string(),
                "server.data_dir" => cfg.server.data_dir = v.as_str().to_string(),
                "network.egress_proxy" => cfg.network.egress_proxy = v.as_str().to_string(),
                "network.extra_ca" => cfg.network.extra_ca = v.as_str().to_string(),
                "network.tz_offset_sec" => cfg.network.tz_offset_sec = v.as_int(),
                "device.app_version" => {
                    // 旧实现 trim 后空串回落默认，不发空版本号
                    cfg.device.app_version = if v.as_str().is_empty() {
                        DEFAULT_APP_VERSION.into()
                    } else {
                        v.as_str().to_string()
                    }
                }
                "device.model" => cfg.device.model = v.as_str().to_string(),
                "device.hw_headers" => {
                    cfg.device.hw_headers = if v.as_str() == "derived" {
                        HwHeaders::Derived
                    } else {
                        HwHeaders::Empty
                    }
                }
                "run.run_mode" => cfg.run.run_mode = v.as_int(),
                "run.step_m" => cfg.run.step_m = v.as_float(),
                "run.bulge_max" => cfg.run.bulge_max = v.as_float(),
                "run.pt_scheme" => {
                    cfg.run.pt_scheme = match v.as_str() {
                        "old" => PtScheme::Old,
                        "new" => PtScheme::New,
                        _ => PtScheme::Auto,
                    }
                }
                "run.fixed_point" => {
                    cfg.run.fixed_point = match v.as_str() {
                        "allnormal" => FixedPoint::AllNormal,
                        "server" => FixedPoint::FollowServer,
                        _ => FixedPoint::Auto,
                    }
                }
                "run.fixed_strict" => cfg.run.fixed_strict = v.as_bool(),
                "run.strict_policy" => cfg.run.strict_policy = v.as_bool(),
                "run.fence_full" => cfg.run.fence_full = v.as_bool(),
                "run.run_area_id" => cfg.run.run_area_id = Some(-1),
                "run.center_lat" => cfg.run.center_lat = Some(v.as_float()),
                "run.center_lng" => cfg.run.center_lng = Some(v.as_float()),
                "run.prefer_stadium" => cfg.run.prefer_stadium = v.as_bool(),
                "run.ignore_daily_limit" => cfg.run.ignore_daily_limit = v.as_bool(),
                "run.dump_policy" => cfg.run.dump_policy = v.as_str().to_string(),
                other => panic!("注册表键 {other} 在 Config::from_values 里没有落点"),
            }
        }
        cfg
    }
}

// ---------------------------------------------------------------------------
// 进程内快照：入口点 store 一次，运行期只读
// ---------------------------------------------------------------------------

static CURRENT: RwLock<Option<Arc<Config>>> = RwLock::new(None);
static FALLBACK: OnceLock<Arc<Config>> = OnceLock::new();

/// 装入进程级快照。入口点（CLI `main` / 未来其他入口）在解析完参数后调用
/// 一次；之后配置只读。重复调用会替换快照，正常运行不会发生。
pub fn store(cfg: Config) {
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(cfg));
}

/// 当前配置。未 [`store`] 时返回内置默认——库函数在任何入口下都不该 panic。
pub fn global() -> Arc<Config> {
    let cur = CURRENT.read().unwrap_or_else(|p| p.into_inner()).clone();
    cur.unwrap_or_else(|| FALLBACK.get_or_init(|| Arc::new(Config::default())).clone())
}

/// 测试注入：替换快照并在 `Drop` 时还原。内部互斥锁让多个用 Guard 的测试
/// 自然串行，不会互相覆盖。
pub struct Guard {
    prev: Option<Arc<Config>>,
    _lock: MutexGuard<'static, ()>,
}

static GUARD_LOCK: Mutex<()> = Mutex::new(());

impl Guard {
    pub fn replace(cfg: Config) -> Guard {
        let lock = GUARD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = CURRENT
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .replace(Arc::new(cfg));
        Guard { prev, _lock: lock }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = self.prev.take();
    }
}

use std::sync::MutexGuard;

// ---------------------------------------------------------------------------
// 打表与启动摘要
// ---------------------------------------------------------------------------

/// `pace-forge config`：整张表。值显示走 [`Config::key_values`]，来源看
/// [`Config::sources`]，口令只显示「已设置」。
pub fn print_table() {
    let cfg = global();
    let kv = cfg.key_values();
    let values: HashMap<&str, &str> = kv.iter().map(|(k, v)| (*k, v.as_str())).collect();
    println!("开关清单（优先级：env > config.toml > 默认）\n");
    for e in TABLE {
        let value = values[e.key];
        let src = cfg.sources.get(e.key).copied().unwrap_or(Source::Default);
        println!("{:>24}（{}）= {}  ← {}", e.key, e.env, value, src.label());
        println!("    默认 {}：{}", e.default_display, e.effect);
    }
    println!(
        "\n以上为全部开关。TOML 写法见 config.example.toml；配置文件里的未知键会在启动时报错，环境变量不受此限。"
    );
    println!("（PACE_OUT、PACE_FAST 这类是 preflight 检查码，不是配置，别混。）");
}

/// 启动时的「非默认项」一行。全默认返回 `None`。
pub fn non_default_line() -> Option<String> {
    let cfg = global();
    let kv = cfg.key_values();
    let values: HashMap<&str, &str> = kv.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut items: Vec<String> = Vec::new();
    for e in TABLE {
        if cfg.sources.get(e.key).copied().unwrap_or(Source::Default) == Source::Default {
            continue;
        }
        items.push(format!("{}={}（{}）", e.env, values[e.key], "非默认"));
    }
    if items.is_empty() {
        None
    } else {
        Some(items.join(", "))
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 注册表与 Config 字段必须一一对应：新增开关少改任何一边都过不了这条。
    #[test]
    fn registry_matches_config_keys() {
        let mut table_keys: Vec<&str> = TABLE.iter().map(|e| e.key).collect();
        table_keys.sort_unstable();
        let mut cfg_keys: Vec<&str> = Config::default()
            .key_values()
            .iter()
            .map(|(k, _)| *k)
            .collect();
        cfg_keys.sort_unstable();
        assert_eq!(
            table_keys, cfg_keys,
            "注册表与 Config::key_values 的键集合不一致"
        );
    }

    #[test]
    fn env_names_are_unique_and_paced_prefixed() {
        let mut names: Vec<&str> = TABLE.iter().map(|e| e.env).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "环境变量名重复");
        for e in TABLE {
            assert!(e.env.starts_with("PACE_"), "{} 不是 PACE_ 前缀", e.env);
            if let Some(legacy) = e.env_legacy {
                assert!(legacy.starts_with("PACE_"), "{legacy} 不是 PACE_ 前缀");
            }
        }
    }

    #[test]
    fn no_file_means_all_defaults() {
        let cfg = load_from(None, &|_| None).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.sources.get("run.bulge_max"), None);
    }

    #[test]
    fn toml_values_reach_fields_with_toml_source() {
        let dir =
            std::env::temp_dir().join(format!("pace-cfg-{}-{:x}", std::process::id(), micros()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[network]\ntz_offset_sec = 0\n\n[run]\nbulge_max = 100\npt_scheme = \"old\"\n",
        )
        .unwrap();
        let cfg = load_from(Some(path.to_str().unwrap()), &|_| None).unwrap();
        assert_eq!(cfg.network.tz_offset_sec, 0);
        assert_eq!(cfg.run.bulge_max, 100.0);
        assert_eq!(cfg.run.pt_scheme, PtScheme::Old);
        assert_eq!(cfg.sources.get("run.bulge_max"), Some(&Source::Toml));
        assert_eq!(
            cfg.sources.get("run.bulge_max"),
            cfg.sources.get("run.pt_scheme")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn env_overrides_toml() {
        let dir =
            std::env::temp_dir().join(format!("pace-cfg-{}-{:x}", std::process::id(), micros()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[run]\nbulge_max = 100\n").unwrap();
        let cfg = load_from(Some(path.to_str().unwrap()), &|name| {
            (name == "PACE_BULGE_MAX").then(|| "200".to_string())
        })
        .unwrap();
        assert_eq!(cfg.run.bulge_max, 200.0);
        assert_eq!(cfg.sources.get("run.bulge_max"), Some(&Source::Env));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_value_and_unknown_key_are_reported_together() {
        let dir =
            std::env::temp_dir().join(format!("pace-cfg-{}-{:x}", std::process::id(), micros()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[run]\nbulge_max = 999\nunknown_key = 1\nstep_m = \"fast\"\n",
        )
        .unwrap();
        let err = load_from(Some(path.to_str().unwrap()), &|_| None).unwrap_err();
        assert!(err.contains("bulge_max"), "应报范围错误: {err}");
        assert!(err.contains("unknown_key"), "应报未知键: {err}");
        assert!(err.contains("step_m"), "应报类型错误: {err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn center_anchor_requires_both_halves() {
        let err = load_from(None, &|name| {
            (name == "PACE_CENTER_LAT").then(|| "30.0".to_string())
        })
        .unwrap_err();
        assert!(err.contains("成对"), "缺一半要报错: {err}");
        let cfg = load_from(None, &|name| match name {
            "PACE_CENTER_LAT" => Some("30.0".into()),
            "PACE_CENTER_LNG" => Some("104.0".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(cfg.run.center_lat, Some(30.0));
        assert_eq!(cfg.run.center_lng, Some(104.0));
    }

    #[test]
    fn legacy_force_normal_point_maps_bool_vocab() {
        let cfg = load_from(None, &|name| {
            (name == "PACE_FORCE_NORMAL_POINT").then(|| "1".to_string())
        })
        .unwrap();
        assert_eq!(cfg.run.fixed_point, FixedPoint::AllNormal);
        let cfg = load_from(None, &|name| {
            (name == "PACE_FORCE_NORMAL_POINT").then(|| "0".to_string())
        })
        .unwrap();
        assert_eq!(cfg.run.fixed_point, FixedPoint::FollowServer);
    }

    #[test]
    fn fixed_point_legacy_loses_to_primary() {
        let cfg = load_from(None, &|name| match name {
            "PACE_FIXED_POINT" => Some("server".into()),
            "PACE_FORCE_NORMAL_POINT" => Some("1".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(cfg.run.fixed_point, FixedPoint::FollowServer);
    }

    #[test]
    fn invalid_env_is_an_error_not_a_fallback() {
        let err = load_from(None, &|name| {
            (name == "PACE_BULGE_MAX").then(|| "6O".to_string())
        })
        .unwrap_err();
        assert!(err.contains("PACE_BULGE_MAX"), "{err}");
    }

    #[test]
    fn empty_env_value_counts_as_unset() {
        // 空串 = 未设（与旧 `filter(|v| !v.is_empty())` 一致）
        let cfg = load_from(None, &|name| {
            (name == "PACE_EGRESS_PROXY").then(|| "".to_string())
        })
        .unwrap();
        assert_eq!(cfg.network.egress_proxy, "");
        assert_eq!(cfg.sources.get("network.egress_proxy"), None);
    }

    /// 与旧实现的字符串语义逐字节对齐：哪些键 trim、哪些保留原文、
    /// 空串回落哪个默认，都是旧代码里各写各的，不能统一掉。
    #[test]
    fn string_semantics_match_legacy_exactly() {
        // token / data_dir / extra_ca / dump_policy：保留原文（旧实现不 trim，
        // 带空格的口令也是合法口令）
        let cfg = load_from(None, &|n| {
            (n == "PACE_FORGE_TOKEN").then(|| "  secret ".to_string())
        })
        .unwrap();
        assert_eq!(cfg.server.token, "  secret ");
        let cfg = load_from(None, &|n| {
            (n == "PACE_FORGE_DATA_DIR").then(|| " /tmp/x ".to_string())
        })
        .unwrap();
        assert_eq!(cfg.server.data_dir, " /tmp/x ");
        let cfg = load_from(None, &|n| {
            (n == "PACE_EXTRA_CA").then(|| " /ca/a.pem ".to_string())
        })
        .unwrap();
        assert_eq!(cfg.network.extra_ca, " /ca/a.pem ");
        // app_version / egress / model：trim（旧实现就是 trim 后用）
        let cfg = load_from(None, &|n| {
            (n == "PACE_APP_VERSION").then(|| " 7.4.21 ".to_string())
        })
        .unwrap();
        assert_eq!(cfg.device.app_version, "7.4.21");
        let cfg = load_from(None, &|n| {
            (n == "PACE_EGRESS_PROXY").then(|| "  http://127.0.0.1:7890  ".to_string())
        })
        .unwrap();
        assert_eq!(cfg.network.egress_proxy, "http://127.0.0.1:7890");
        // app_version 空串回落默认（旧 trim+filter → 默认，不发空版本号）
        let cfg = load_from(None, &|n| {
            (n == "PACE_APP_VERSION").then(|| " ".to_string())
        })
        .unwrap();
        assert_eq!(cfg.device.app_version, DEFAULT_APP_VERSION);
    }

    /// 数值边界与旧实现的 filter/parse 范围对齐；界外从「静默回落」改为报错。
    #[test]
    fn numeric_boundaries_match_legacy() {
        let ok = |name: &str, raw: &str| {
            load_from(None, &|n| (n == name).then(|| raw.to_string())).unwrap()
        };
        let err = |name: &str, raw: &str| {
            load_from(None, &|n| (n == name).then(|| raw.to_string())).is_err()
        };

        // tz ±20 小时（旧 filter 同范围：界内生效、界外回落默认 → 现在报错）
        assert_eq!(
            ok("PACE_TZ_OFFSET_SEC", "-72000").network.tz_offset_sec,
            -72_000
        );
        assert_eq!(
            ok("PACE_TZ_OFFSET_SEC", "72000").network.tz_offset_sec,
            72_000
        );
        assert_eq!(
            ok("PACE_TZ_OFFSET_SEC", "28800").network.tz_offset_sec,
            28_800
        );
        assert!(err("PACE_TZ_OFFSET_SEC", "-72001"));
        assert!(err("PACE_TZ_OFFSET_SEC", "72001"));
        assert!(err("PACE_TZ_OFFSET_SEC", "abc"));

        // bulge 0~300（旧 filter 同范围）
        assert_eq!(ok("PACE_BULGE_MAX", "0").run.bulge_max, 0.0);
        assert_eq!(ok("PACE_BULGE_MAX", "300").run.bulge_max, 300.0);
        assert_eq!(ok("PACE_BULGE_MAX", "60.5").run.bulge_max, 60.5);
        assert!(err("PACE_BULGE_MAX", "-0.1"));
        assert!(err("PACE_BULGE_MAX", "300.1"));
        assert!(err("PACE_BULGE_MAX", "inf"));
        assert!(err("PACE_BULGE_MAX", "6O"));

        // step_m：正数全放开（旧任意 f64，只挡明显打错的 0/负数/inf）
        assert_eq!(ok("PACE_STEP_M", "0.001").run.step_m, 0.001);
        assert_eq!(ok("PACE_STEP_M", "100000").run.step_m, 100_000.0);
        assert!(err("PACE_STEP_M", "0"));
        assert!(err("PACE_STEP_M", "-5"));
        assert!(err("PACE_STEP_M", "inf"));

        // run_mode：任意整数（服务端 0/2~6 实验值不设限）
        assert_eq!(ok("PACE_RUN_MODE", "0").run.run_mode, 0);
        assert_eq!(ok("PACE_RUN_MODE", "6").run.run_mode, 6);
        assert_eq!(ok("PACE_RUN_MODE", "-3").run.run_mode, -3);
        assert!(err("PACE_RUN_MODE", "abc"));
        assert!(err("PACE_RUN_MODE", "2.5"));
        // 旧实现对原文精确 parse：" 1" 回落默认，现在报错
        assert!(err("PACE_RUN_MODE", " 1"));

        // lat/lng 边界（必须成对设置，缺一半本身会报错）
        let pair = |lat: &str, lng: &str| {
            load_from(None, &|n| match n {
                "PACE_CENTER_LAT" => Some(lat.to_string()),
                "PACE_CENTER_LNG" => Some(lng.to_string()),
                _ => None,
            })
            .unwrap()
        };
        let pair_err = |lat: &str, lng: &str| {
            load_from(None, &|n| match n {
                "PACE_CENTER_LAT" => Some(lat.to_string()),
                "PACE_CENTER_LNG" => Some(lng.to_string()),
                _ => None,
            })
            .is_err()
        };
        assert_eq!(pair("-90", "104").run.center_lat, Some(-90.0));
        assert_eq!(pair("90", "104").run.center_lat, Some(90.0));
        assert_eq!(pair("30", "-180").run.center_lng, Some(-180.0));
        assert_eq!(pair("30", "180").run.center_lng, Some(180.0));
        assert!(pair_err("90.1", "104"));
        assert!(pair_err("nan", "104"));
        assert!(pair_err("30", "180.1"));
    }

    /// 布尔词汇表：统一后的 0/1、true/false、yes/no 全部可用（大小写不敏感）。
    #[test]
    fn bool_vocab_is_unified_and_case_insensitive() {
        let val = |name: &str, raw: &str| {
            load_from(None, &|n| (n == name).then(|| raw.to_string()))
                .unwrap()
                .run
                .ignore_daily_limit
        };
        for t in ["1", "true", "yes", "TRUE", "True", "YES"] {
            assert!(val("PACE_IGNORE_DAILY_LIMIT", t), "{t} 应为 true");
        }
        for f in ["0", "false", "no", "FALSE", "No"] {
            assert!(!val("PACE_IGNORE_DAILY_LIMIT", f), "{f} 应为 false");
        }
        assert!(
            load_from(None, &|n| {
                (n == "PACE_IGNORE_DAILY_LIMIT").then(|| "on".to_string())
            })
            .is_err()
        );
        // 语义修正对照：旧实现连 "0" 都会绕过日上限（存在性判断）
        assert!(!val("PACE_IGNORE_DAILY_LIMIT", "0"));
    }

    /// 枚举键词汇表与旧实现精确对齐（原文匹配，不 trim）。
    #[test]
    fn choice_and_legacy_vocab_exact() {
        let fp = |raw: &str| {
            load_from(None, &|n| {
                (n == "PACE_FIXED_POINT").then(|| raw.to_string())
            })
            .unwrap()
            .run
            .fixed_point
        };
        assert_eq!(fp("auto"), FixedPoint::Auto);
        assert_eq!(fp("all0"), FixedPoint::AllNormal);
        assert_eq!(fp("allnormal"), FixedPoint::AllNormal);
        assert_eq!(fp("server"), FixedPoint::FollowServer);
        assert_eq!(fp("follow"), FixedPoint::FollowServer);
        assert!(
            load_from(None, &|n| (n == "PACE_FIXED_POINT")
                .then(|| "1".to_string()))
            .is_err()
        );
        assert!(
            load_from(None, &|n| (n == "PACE_FIXED_POINT")
                .then(|| " all0 ".to_string()))
            .is_err()
        );

        // 旧变量词汇：0/1/false/no/true/yes
        let legacy = |raw: &str| {
            load_from(None, &|n| {
                (n == "PACE_FORCE_NORMAL_POINT").then(|| raw.to_string())
            })
            .unwrap()
            .run
            .fixed_point
        };
        assert_eq!(legacy("0"), FixedPoint::FollowServer);
        assert_eq!(legacy("false"), FixedPoint::FollowServer);
        assert_eq!(legacy("no"), FixedPoint::FollowServer);
        assert_eq!(legacy("1"), FixedPoint::AllNormal);
        assert_eq!(legacy("true"), FixedPoint::AllNormal);
        assert_eq!(legacy("yes"), FixedPoint::AllNormal);
        assert!(
            load_from(None, &|n| {
                (n == "PACE_FORCE_NORMAL_POINT").then(|| "2".to_string())
            })
            .is_err()
        );

        let ps = |raw: &str| {
            load_from(None, &|n| (n == "PACE_PT_SCHEME").then(|| raw.to_string()))
                .unwrap()
                .run
                .pt_scheme
        };
        assert_eq!(ps("old"), PtScheme::Old);
        assert_eq!(ps("marks"), PtScheme::Old);
        assert_eq!(ps("new"), PtScheme::New);
        assert_eq!(ps("auto"), PtScheme::Auto);
        // 旧实现对任何未知值（含拼写错误）静默走 auto；现在报错
        assert!(
            load_from(None, &|n| (n == "PACE_PT_SCHEME")
                .then(|| "odl".to_string()))
            .is_err()
        );

        let cfg = load_from(None, &|n| {
            (n == "PACE_RUN_AREA_ID").then(|| "minus1".to_string())
        })
        .unwrap();
        assert_eq!(cfg.run.run_area_id, Some(-1));
        assert!(
            load_from(None, &|n| (n == "PACE_RUN_AREA_ID")
                .then(|| "0".to_string()))
            .is_err()
        );

        let hw = |raw: &str| {
            load_from(None, &|n| {
                (n == "PACE_DEVICE_HW_HEADERS").then(|| raw.to_string())
            })
            .unwrap()
            .device
            .hw_headers
        };
        assert_eq!(hw("empty"), HwHeaders::Empty);
        assert_eq!(hw("0"), HwHeaders::Empty);
        assert_eq!(hw("derived"), HwHeaders::Derived);
        assert_eq!(hw("1"), HwHeaders::Derived);
        assert_eq!(hw("true"), HwHeaders::Derived);
        assert!(
            load_from(None, &|n| {
                (n == "PACE_DEVICE_HW_HEADERS").then(|| "yes".to_string())
            })
            .is_err()
        );
    }

    /// TOML 里的数组 / datetime / 嵌套意外形态要报错，不能静默丢键。
    #[test]
    fn toml_array_values_are_rejected() {
        let dir =
            std::env::temp_dir().join(format!("pace-cfg-{}-{:x}", std::process::id(), micros()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[run]\npt_scheme = [\"old\"]\n").unwrap();
        let err = load_from(Some(path.to_str().unwrap()), &|_| None).unwrap_err();
        assert!(err.contains("pt_scheme"), "{err}");
        assert!(err.contains("数组"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// run_area_id 的合法值只有 -1/minus1；结构体字段必须是 Some(-1)。
    #[test]
    fn run_area_id_default_is_none() {
        let cfg = load_from(None, &|_| None).unwrap();
        assert_eq!(cfg.run.run_area_id, None);
    }

    #[test]
    fn toml_scalar_types_are_accepted() {
        let dir =
            std::env::temp_dir().join(format!("pace-cfg-{}-{:x}", std::process::id(), micros()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        // TOML 原生整数/布尔也要能落到浮点/布尔键上
        std::fs::write(&path, "[run]\nstep_m = 7\nfixed_strict = true\n").unwrap();
        let cfg = load_from(Some(path.to_str().unwrap()), &|_| None).unwrap();
        assert_eq!(cfg.run.step_m, 7.0);
        assert!(cfg.run.fixed_strict);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn guard_replaces_and_restores() {
        assert_eq!(global().run.bulge_max, 60.0);
        {
            let mut cfg = Config::default();
            cfg.run.bulge_max = 250.0;
            let _g = Guard::replace(cfg);
            assert_eq!(global().run.bulge_max, 250.0);
        }
        assert_eq!(global().run.bulge_max, 60.0);
    }

    #[test]
    fn secret_tokens_are_masked_in_key_values() {
        let mut cfg = Config::default();
        cfg.server.token = "hunter2".into();
        let kv: HashMap<&str, String> = cfg.key_values().into_iter().collect();
        assert_eq!(kv["server.token"], "已设置（不显示值）");
        assert!(
            !kv.values().any(|v| v.contains("hunter2")),
            "口令原文不许出现在任何显示值里"
        );
    }

    #[test]
    fn explicit_missing_path_is_an_error() {
        assert!(load_from(Some("/nonexistent/pace-cfg.toml"), &|_| None).is_err());
    }

    fn micros() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0)
    }
}
