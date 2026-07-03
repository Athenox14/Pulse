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
    "https://en.wikipedia.org/wiki/Special:Random",
    "https://news.ycombinator.com/",
    "https://github.com/trending",
    "https://www.reddit.com/",
    "https://www.bbc.com/news",
    "https://techcrunch.com/",
    "https://www.wikipedia.org/",
    "https://www.w3.org/",
    "https://stackoverflow.com/questions",
    "https://www.producthunt.com/",
    "https://lobste.rs/",
    "https://www.theverge.com/",
    "https://arstechnica.com/",
    "https://www.wired.com/",
    "https://www.reuters.com/",
    "https://www.nytimes.com/",
    "https://www.theguardian.com/international",
    "https://www.cnn.com/",
    "https://www.npr.org/",
    "https://en.wikipedia.org/wiki/List_of_most_popular_websites",
    "https://en.wikipedia.org/wiki/List_of_Internet_top-level_domains",
    "https://dmoz-odp.org/",
    "https://www.dmoz-odp.org/Computers/",
    "https://curlie.org/",
    "https://awesome.re/",
    "https://github.com/sindresorhus/awesome",
    "https://en.wikipedia.org/wiki/Category:Internet_properties_established_in_2020",
    "https://www.similarweb.com/top-websites/",
    "https://moz.com/top500",
    "https://www.alexa.com/topsites",
    "https://www.crunchbase.com/discover/organization.companies",
    "https://opensource.org/licenses",
    "https://www.w3schools.com/",
    "https://developer.mozilla.org/en-US/",
    "https://stackshare.io/",
    "https://www.g2.com/",
    "https://www.capterra.com/",
    "https://alternativeto.net/",
    "https://www.producthunt.com/topics",
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

    WIKI_RANDOM = "https://en.wikipedia.org/wiki/Special:Random"
    stall_refills = 0

    while len(seen_domains) < TARGET_COUNT:
        if not queue:
            # Big sites often sit behind bot walls and stop yielding links;
            # Wikipedia's random-article endpoint never runs dry and every
            # article links out to dozens of external domains (refs, ISBNs,
            # official sites, archive.org, etc.) so it works as an infinite
            # source of fresh domains once the curated seed list is exhausted.
            queue.extend([WIKI_RANDOM] * 50)
            stall_refills += 1
            print(f"queue drained, refilling with Wikipedia random ({stall_refills})")
            if stall_refills > 500:
                print("too many empty refills, giving up early")
                break

        page = queue.popleft()
        if page != WIKI_RANDOM and page in visited_pages:
            continue
        if page != WIKI_RANDOM:
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
            if len(visited_pages) < 6000:
                queue.append(link)

        time.sleep(0.05)  # be polite but don't crawl at a snail's pace for 10k targets

    print(f"Done. Added {added} new monitors, total distinct domains: {len(seen_domains)}")


if __name__ == "__main__":
    crawl()
