//! 云服务地址的安全判定（**纯逻辑**，宿主机上可测）。
//!
//! 单独成文件的原因和 `saves` / `request` 一样：`src/host/` 下的东西在宿主机上根本编不到，
//! 而「这个地址能不能把设备凭证送过去」是能直接判定对错的语义，必须有本地守护用例。
//!
//! 为什么需要这一层：`packs/cloud-config.json` 随插件目录分发，而插件目录是可写的
//! （宿主更新插件时会整个重写它，用户和其它插件也能碰）。谁改了这个地址，谁就能收到
//! 我们每次请求带的 `Authorization: Bearer <设备凭证>`。所以「必须是 https」还不够 ——
//! `https://127.0.0.1:8443` 一样能把凭证交给本机上的任意监听服务。

/// 主机名是否指向公网。
///
/// 按**字面量**判断，不做 DNS 解析：解析后再比对存在 TOCTOU 窗口，而且「一个正常域名
/// 解析到内网」本身就是 DNS rebinding 的形态，插件侧无法可靠兜住。这里能守住的是
/// 「配置被改成 IP 字面量 / localhost」这一类 —— 也就是真实可被利用的那一类。
pub fn is_public_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() {
        return false;
    }
    // 去掉 IPv6 zone id（`fe80::1%eth0`）与 FQDN 的尾点（`127.0.0.1.`）。
    // 不做这一步，`127.0.0.1.` 会掉进「普通域名」分支被放行 —— 这是本地用例抓到的。
    let host = host.split('%').next().unwrap_or(host).trim_end_matches('.');
    if host.is_empty() {
        return false;
    }
    let lower = host.to_ascii_lowercase();
    // `localhost` 及其子域（`foo.localhost` 按 RFC 6761 同样是回环语义）。
    if lower == "localhost" || lower.ends_with(".localhost") {
        return false;
    }
    if let Ok(v4) = lower.parse::<std::net::Ipv4Addr>() {
        return is_public_v4(v4);
    }
    if lower.contains(':') {
        // 带冒号却不是合法 IPv6：当作不可用（别让畸形写法绕过判定）。
        return lower.parse::<std::net::Ipv6Addr>().is_ok_and(is_public_v6);
    }
    // 形如 IPv4 却解析不了（`127.0.0.1.1`、`1.2.3`、`999.1.1.1`）：
    // 各家解析器对这类写法解释不一，插件不该赌，直接拒绝。
    if lower.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return false;
    }
    // 普通域名：按名字放行（主机侧另有 TLS 校验）。
    true
}

fn is_public_v4(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
        // 100.64.0.0/10：运营商级 NAT，不是公网单播。
        || (a == 100 && (64..128).contains(&b))
        // 0.0.0.0/8 本网络、192.0.0.0/24 IETF 协议分配、198.18/15 基准测试、240/4 保留。
        || a == 0
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240)
}

fn is_public_v6(ip: std::net::Ipv6Addr) -> bool {
    // IPv4 映射地址（`::ffff:127.0.0.1`）按内层 IPv4 判，否则回环会从这条路漏过去。
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // fc00::/7 唯一本地地址；fe80::/10 链路本地。
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 公网域名放行() {
        for host in ["saves.miband.qihe0507.top", "example.com", "a.b.c", "8.8.8.8", "1.1.1.1"] {
            assert!(is_public_host(host), "{host} 是公网地址，应当放行");
        }
    }

    #[test]
    fn 回环与_localhost_一律拒绝() {
        for host in [
            "localhost", "LOCALHOST", "foo.localhost", "127.0.0.1", "127.1.2.3", "[::1]", "::1",
        ] {
            assert!(!is_public_host(host), "{host} 指向本机，必须拒绝");
        }
    }

    #[test]
    fn 私有与保留网段一律拒绝() {
        for host in [
            // 私有
            "10.0.0.1", "172.16.0.1", "172.31.255.254", "192.168.1.1",
            // 链路本地 / 云元数据
            "169.254.169.254",
            // 运营商级 NAT
            "100.64.0.1",
            // 文档 / 基准测试 / 保留
            "192.0.2.1", "198.18.0.1", "240.0.0.1", "0.0.0.0",
            // IPv6 私有与链路本地
            "fc00::1", "fd00::1", "fe80::1",
            // 映射回来的回环
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_host(host), "{host} 不是公网地址，必须拒绝");
        }
    }

    #[test]
    fn 空主机与畸形冒号地址拒绝() {
        assert!(!is_public_host(""));
        // 带 zone id 的回环 / 链路本地：去掉 `%` 之后仍必须是私网判定。
        assert!(!is_public_host("127.0.0.1%eth0"));
        assert!(!is_public_host("fe80::1%eth0"));
        // 带尾点的 FQDN 写法也是同一个地址。
        assert!(!is_public_host("127.0.0.1."));
        assert!(!is_public_host("localhost."));
        // 「看着像 IPv4 但解析不了」的一律拒绝，不赌解析器的方言。
        for host in ["1.2.3", "127.0.0.1.1", "999.1.1.1", "."] {
            assert!(!is_public_host(host), "{host} 不是合法公网地址，必须拒绝");
        }
    }

    /// 生产地址必须始终通过 —— 这条是「别把正常功能一起改坏」的护栏。
    #[test]
    fn 生产云地址通过() {
        assert!(is_public_host("saves.miband.qihe0507.top"));
    }
}
