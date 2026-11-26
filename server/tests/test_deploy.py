from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_compose_binds_host_network_without_a_tunnel():
    compose = (ROOT / "docker-compose.yml").read_text(encoding="utf-8")
    lowered = compose.lower()
    assert "network_mode: host" in compose
    assert "cloudflare" not in lowered
    assert "ngrok" not in lowered
    assert "tailscale" not in lowered
    dockerfile = (ROOT / "Dockerfile").read_text(encoding="utf-8")
    assert "8080" in dockerfile


def test_deploy_doc_is_chinese_and_covers_ipv6_http_and_https():
    text = (ROOT / "docs" / "deploy-raspberry-pi.md").read_text(encoding="utf-8")
    assert "树莓派" in text
    assert "IPv6" in text
    assert "[2001:db8::1]" in text or "方括号" in text
    assert "明文" in text
    assert "Let’s Encrypt" in text or "Let's Encrypt" in text
    assert "AAAA" in text
    assert "不使用 Cloudflare Tunnel" in text
    assert "内网穿透" in text
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert "遥显" in readme
    assert "pytest" in readme
