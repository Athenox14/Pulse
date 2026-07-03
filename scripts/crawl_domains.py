"""
Crawl web BFS from seed pages, extract distinct domains, register each as an
HTTP monitor in Pulse via its API, until TARGET_COUNT monitors exist.

Usage: python crawl_domains.py [target_count]
"""
import re
import sys
import time
import requests
from collections import deque
from urllib.parse import urlparse, urljoin

API_BASE = "http://127.0.0.1:3939"
TARGET_COUNT = int(sys.argv[1]) if len(sys.argv) > 1 else 1000

SEEDS = [
    "https://en.wikipedia.org/wiki/Main_Page",
    "https://news.ycombinator.com/",
    "https://github.com/trending",
    "https://www.reddit.com/",
    "https://www.bbc.com/news",
    "https://techcrunch.com/",
    "https://www.wikipedia.org/",
    "https://www.w3.org/",
    "https://stackoverflow.com/questions",
]

HREF_RE = re.compile(r'href=["\'](https?://[^"\'>\s]+)', re.IGNORECASE)
HEADERS = {"User-Agent": "PulseCrawler/1.0 (+monitoring discovery bot)"}


def existing_domains():
    try:
        r = requests.get(f"{API_BASE}/api/monitors", timeout=5)
        r.raise_for_status()
        return {urlparse(m["url"]).netloc for m in r.json() if m.get("url")}
    except Exception as e:
        print(f"warn: could not fetch existing monitors: {e}")
        return set()


def add_monitor(domain):
    payload = {
        "name": domain,
        "type": "http",
        "url": f"https://{domain}",
        "interval_sec": 300,
        "retries": 1,
        "timeout_sec": 10,
    }
    try:
        r = requests.post(f"{API_BASE}/api/monitors", json=payload, timeout=5)
        return r.status_code in (200, 201)
    except Exception as e:
        print(f"  add failed for {domain}: {e}")
        return False


def crawl():
    seen_domains = existing_domains()
    print(f"Already have {len(seen_domains)} monitor domains.")
    visited_pages = set()
    queue = deque(SEEDS)
    added = 0

    while queue and len(seen_domains) < TARGET_COUNT:
        page = queue.popleft()
        if page in visited_pages:
            continue
        visited_pages.add(page)

        try:
            resp = requests.get(page, headers=HEADERS, timeout=8)
            html = resp.text
        except Exception as e:
            print(f"skip {page}: {e}")
            continue

        links = HREF_RE.findall(html)
        for link in links:
            domain = urlparse(link).netloc.lower()
            if not domain or domain in seen_domains:
                continue
            seen_domains.add(domain)
            if add_monitor(domain):
                added += 1
                print(f"[{len(seen_domains)}/{TARGET_COUNT}] + {domain}")
            if len(seen_domains) >= TARGET_COUNT:
                break
            # keep crawling breadth-first through discovered pages too
            if len(visited_pages) < 400:
                queue.append(link)

        time.sleep(0.2)  # be polite

    print(f"Done. Added {added} new monitors, total distinct domains: {len(seen_domains)}")


if __name__ == "__main__":
    crawl()
