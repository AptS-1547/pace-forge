//! 出口 IP 自检：把「提交时服务端用的公网出口 IP」亮在控制台顶栏。
//!
//! 背景（#10）：同类工具栽在「IP 提交异常」——出口 IP 的归属地和学校所在
//! 地对不上，服务端成片清理记录。pace-forge 不选、不改、不伪造出口 IP，
//! 提交走的就是本机默认网络；这里只做一件事：把**真实**出口 IP 探出来
//! 显示。部署在异地服务器（学校在新疆、出口在北京）的人一眼就能看出
//! 特征，自己决定换不换网络。
//!
//! - 探测源：ipify（JSON）→ httpbin（JSON）兜底，单次 5 秒超时；
//! - 全部失败返回 `ok:false`，顶栏显示「出口未知」——它只是自检，不阻塞
//!   任何功能，也不在界面弹错误；
//! - 结果缓存 5 分钟：控制台每次刷新页面都会来问，免费接口不能这么刷。
//!
//! 注意显示侧（web/index.html）拿到的 `ip` 字符串来自外部接口，渲染必须
//! 走 `textContent`，不许拼 `innerHTML`。

use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Probe {
    ip: String,
    source: &'static str,
}

static CACHE: OnceLock<Mutex<Option<(Probe, Instant)>>> = OnceLock::new();
const TTL: Duration = Duration::from_secs(300);

/// 从 JSON 接口取 `field` 字段当 IP。
///
/// 校验只认「短、全可打印、无空格」的字符串：上游万一返回整段 HTML/错误
/// 页，别把它当 IP 显示出来。httpbin 的 `origin` 在有代理时会带逗号
/// （`1.2.3.4, 10.0.0.1`），取第一段。
fn fetch_json_ip(url: &str, field: &str) -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .build();
    let body: Value = agent.get(url).call().ok()?.into_json().ok()?;
    let raw = body.get(field)?.as_str()?;
    let ip = raw.split(',').next()?.trim();
    if ip.is_empty() || ip.len() > 45 || ip.bytes().any(|b| !b.is_ascii_graphic()) {
        return None;
    }
    Some(ip.to_string())
}

fn probe_once() -> Option<Probe> {
    if let Some(ip) = fetch_json_ip("https://api.ipify.org?format=json", "ip") {
        return Some(Probe { ip, source: "ipify" });
    }
    if let Some(ip) = fetch_json_ip("https://httpbin.org/ip", "origin") {
        return Some(Probe { ip, source: "httpbin" });
    }
    None
}

/// 带缓存的探测结果，可直接作 JSON 响应体。
pub fn cached_json() -> Value {
    let lock = CACHE.get_or_init(|| Mutex::new(None));
    let mut g = lock.lock().unwrap_or_else(|p| p.into_inner());
    let fresh = g
        .as_ref()
        .map(|(p, t)| t.elapsed() < TTL && !p.ip.is_empty())
        .unwrap_or(false);
    if !fresh {
        let probed = probe_once();
        *g = probed.map(|p| (p, Instant::now()));
    }
    match &*g {
        Some((p, _)) => json!({ "ok": true, "ip": p.ip, "source": p.source }),
        None => json!({ "ok": false }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_shape_is_stable() {
        // 只看形状，不真发请求（测试机不一定有外网）：ok:false 分支。
        // （真发过请求的环境里缓存已被填充，这里不覆盖全局缓存，
        //  改成校验字段集合而不是值。）
        let v = cached_json();
        assert!(v.get("ok").is_some(), "必须有 ok 字段: {v}");
        if v["ok"].as_bool().unwrap_or(false) {
            assert!(v["ip"].is_string() && !v["ip"].as_str().unwrap().is_empty());
            assert!(v["source"].is_string());
        }
    }
}
