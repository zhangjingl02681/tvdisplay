import pytest

from tvdisplay.config import PLACEHOLDER_PASSWORDS, load_settings
from tvdisplay.healthcheck import health_urls


def test_load_settings_reads_host_port_and_directory(tmp_path):
    settings = load_settings(
        {
            "ADMIN_PASSWORD": "a-long-random-secret",
            "HOST": "::",
            "PORT": "8080",
            "DATA_DIR": str(tmp_path),
        }
    )
    assert settings.host == "::"
    assert settings.port == 8080
    assert settings.data_dir == tmp_path
    assert settings.admin_password == "a-long-random-secret"


def test_missing_password_exits_without_leaking_a_value():
    with pytest.raises(SystemExit) as caught:
        load_settings({})
    assert "ADMIN_PASSWORD" in str(caught.value)
    assert "change-me" not in str(caught.value)


@pytest.mark.parametrize("placeholder", sorted(PLACEHOLDER_PASSWORDS))
def test_placeholder_passwords_are_rejected(placeholder, tmp_path):
    with pytest.raises(SystemExit) as caught:
        load_settings({"ADMIN_PASSWORD": placeholder, "DATA_DIR": str(tmp_path)})
    assert placeholder not in str(caught.value)


def test_bad_port_is_rejected():
    with pytest.raises(SystemExit):
        load_settings({"ADMIN_PASSWORD": "a-long-random-secret", "PORT": "nope"})


def test_health_urls_cover_ipv6_and_localhost():
    assert health_urls("::", "8080") == [
        "http://[::1]:8080/health",
        "http://127.0.0.1:8080/health",
    ]
    assert health_urls("127.0.0.1", "8080") == ["http://127.0.0.1:8080/health"]
    assert health_urls("2001:db8::1", "9090") == ["http://[2001:db8::1]:9090/health"]
