"""本地健康检查用的探测地址。"""

from __future__ import annotations

import os
import urllib.request


def health_urls(host: str, port: str) -> list[str]:
    if host in {"::", "0.0.0.0", ""}:
        return [f"http://[::1]:{port}/health", f"http://127.0.0.1:{port}/health"]
    if ":" in host:
        return [f"http://[{host}]:{port}/health"]
    return [f"http://{host}:{port}/health"]


def main() -> None:
    host = os.environ.get("HOST", "::")
    port = os.environ.get("PORT", "8080")
    for url in health_urls(host, port):
        try:
            with urllib.request.urlopen(url, timeout=3) as response:
                if response.status == 200:
                    raise SystemExit(0)
        except SystemExit:
            raise
        except Exception:
            continue
    raise SystemExit(1)


if __name__ == "__main__":
    main()
