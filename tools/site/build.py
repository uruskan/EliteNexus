"""Build Elite Nexus's static GitHub Pages site. Python standard library only."""
from pathlib import Path
import argparse
import html
import json
import re
import shutil

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'site'
CONTENT = Path(__file__).parent / 'content'
BASE = 'https://uruskan.github.io/EliteNexus/'
REPO = 'https://github.com/uruskan/EliteNexus'
DOWNLOAD = REPO + '/releases/latest'
SPONSOR = 'https://github.com/sponsors/uruskan'
VERSION = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(encoding='utf-8'), re.M).group(1)
# What the app does, for the SoftwareApplication schema that search engines and AI assistants read.
FEATURES = [
    'Live market data from EDDN: commodity search, sell-my-cargo, trade routes, pinned orders, a run planner and price alerts',
    'Exploration body values, Observatory-style highlights and Road to Riches tours',
    'Exobiology sampling radar, species predictions and a first-logged bonus hunt',
    'Spansh neutron, exact and fleet-carrier route planners, with the next system copied to the clipboard',
    'HUD colour editor that keeps hostile red, friendly green and shield blue',
    'Cockpit light dimmer and Odyssey glare and exposure presets',
    'Engineering blueprint wishlist, materials shopping list and material-trader plan',
    'Fleet loadouts, missions and massacre stacks, mining, fleet carrier, colonisation and BGS tracking',
    'An overlay at the top of the game screen: next jump, arrivals, approach, sampling distance, Rhino deposits, trade stops',
    'AI co-pilot grounded in your own ship, journal and market data',
    'Opt-in sync with EDDN, EDSM, Inara and EDAstro',
]
KEYWORDS = 'Elite Dangerous, companion app, EDDN, exobiology, exploration, trading, HUD colours, cockpit lights, Odyssey glare, engineering, fleet carrier, Spansh, EDDiscovery alternative, EDMC alternative'

def faq_schema(body):
    """The page's plain <details><summary> questions as an FAQPage schema, or None."""
    text = lambda s: html.unescape(re.sub(r'<[^>]+>', '', s)).strip()
    items = [{'@type': 'Question', 'name': text(q), 'acceptedAnswer': {'@type': 'Answer', 'text': text(a)}}
             for q, a in re.findall(r'<details>\s*<summary>(.*?)</summary>(.*?)</details>', body, re.S)]
    return {'@context': 'https://schema.org', '@type': 'FAQPage', 'mainEntity': items} if items else None

def escape(value):
    return html.escape(str(value), quote=True)

def shot(filename, alt, caption, eager=False):
    """A screenshot is a plain link to its full-size file: nothing to learn, nothing to break."""
    return (f'<figure class="screenshot"><a href="screenshots/{escape(filename)}" aria-label="View the full-size screenshot: {escape(alt)}">'
            f'<img src="screenshots/{escape(filename)}" width="1480" height="920" alt="{escape(alt)}" loading="{"eager" if eager else "lazy"}" decoding="async"></a>'
            f'<figcaption>{escape(caption)}</figcaption></figure>')

def header(slug):
    def link(href, label):
        active = ' aria-current="page"' if href == slug + '.html' else ''
        return f'<a href="{href}"{active}>{label}</a>'
    return f'''<a class="skip-link" href="#main">Skip to content</a>
<header class="site-header"><div class="header-inner">
<a class="brand" href="index.html" aria-label="Elite Nexus home"><span class="brand-mark" aria-hidden="true"></span><span>Elite Nexus</span></a>
<button class="menu-toggle" aria-expanded="false" aria-controls="primary-nav" aria-label="Open navigation"><span></span><span></span><span></span></button>
<nav id="primary-nav" class="primary-nav" aria-label="Main navigation">
{link('index.html#features', 'Features')}
<details class="nav-dropdown"><summary>Cockpit <span aria-hidden="true">⌄</span></summary><div class="dropdown-panel">{link('hud-colours.html', 'HUD colours')}{link('cockpit-lights.html', 'Cockpit lights')}{link('odyssey-glare.html', 'Glare &amp; exposure')}</div></details>
{link('trading.html', 'Trading')}
<details class="nav-dropdown"><summary>Explore <span aria-hidden="true">⌄</span></summary><div class="dropdown-panel">{link('exploration.html', 'Exploration &amp; navigation')}{link('exobiology.html', 'Exobiology')}</div></details>
<details class="nav-dropdown"><summary>Commander <span aria-hidden="true">⌄</span></summary><div class="dropdown-panel">{link('commander.html', 'Fleet &amp; engineering')}{link('companion.html', 'AI, voice &amp; your data')}</div></details>
{link('compare.html', 'Compare')}
<a class="nav-source" href="{REPO}">GitHub <span aria-hidden="true">↗</span></a>
<a class="button button-small" href="{DOWNLOAD}">Download <span aria-hidden="true">↓</span></a>
</nav></div></header>'''

def footer():
    return f'''<footer class="site-footer"><div class="container footer-grid">
<div class="footer-brand"><a class="brand" href="index.html"><span class="brand-mark" aria-hidden="true"></span><span>Elite Nexus</span></a><p>One app. Your whole galaxy.</p><span class="small-label">Free software · Native app for Windows, Linux and macOS</span></div>
<div><h2>Features</h2><a href="hud-colours.html">HUD colours</a><a href="cockpit-lights.html">Cockpit lights</a><a href="odyssey-glare.html">Glare &amp; exposure</a><a href="trading.html">Trading &amp; markets</a><a href="exploration.html">Exploration &amp; navigation</a><a href="exobiology.html">Exobiology</a><a href="commander.html">Fleet &amp; engineering</a><a href="companion.html">AI, voice &amp; your data</a></div>
<div><h2>Project</h2><a href="{DOWNLOAD}">Download</a><a href="{REPO}/releases">Release notes</a><a href="{REPO}/blob/main/docs/USER_GUIDE.md">User guide</a><a href="{REPO}">Source on GitHub</a><a href="{REPO}/issues">Report an issue</a><a href="{SPONSOR}">Sponsor on GitHub</a><a href="compare.html">Compared with other tools</a><a href="thanks.html">Thanks</a><a href="{REPO}/blob/main/LICENSE">GPL-3.0 licence</a></div>
</div><div class="container footer-bottom"><p>An independent community companion. Not affiliated with or endorsed by Frontier Developments.<br>Elite Dangerous is a trademark of Frontier Developments plc.</p><a href="#top">Back to top ↑</a></div></footer>'''

def page_body(page):
    if page['slug'] == 'index':
        return (CONTENT / 'index.html').read_text(encoding='utf-8')
    body = (CONTENT / (page['slug'] + '.html')).read_text(encoding='utf-8')
    jump_links = [f'<a href="#{escape(item["id"])}">{escape(item["label"])}</a>' for item in page.get('toc', [])]
    if not jump_links:
        for anchor, section in re.findall(r'<section\b[^>]*\bid=["\x27]([^"\x27]+)["\x27][^>]*>(.*?)(?=<section\b|$)', body, re.S):
            heading = re.search(r'<h2\b[^>]*>(.*?)</h2>', section, re.S)
            if heading:
                jump_links.append(f'<a href="#{escape(anchor)}">{re.sub("<[^>]+>", "", heading.group(1))}</a>')
    jumps = '<nav class="page-jump container" aria-label="On this page"><span>ON THIS PAGE</span>' + ''.join(jump_links) + '</nav>' if jump_links else ''
    hero = f'''<section class="page-hero container"><div><a class="back-link" href="index.html#features">← All features</a><p class="eyebrow">{escape(page['kicker'])}</p><h1>{escape(page['heading'])}</h1><p class="lead">{escape(page['lead'])}</p></div><div class="page-hero-image">{shot(page['hero_image'], page['hero_alt'], page['hero_caption'], True)}</div></section>'''
    # One next step per page, instead of a grid of related links that send readers round in circles.
    following = page['next']
    closing = f'''<section class="container page-next"><p class="eyebrow">Next</p><a class="next-link" href="{escape(following['href'])}"><h2>{escape(following['label'])} <span aria-hidden="true">→</span></h2><p>{escape(following['description'])}</p></a></section>'''
    return hero + jumps + body + closing

def canonical(target):
    """The canonical URL of a site-relative link such as 'index.html#features'."""
    path = target.split('#')[0]
    return BASE + ('' if path == 'index.html' else path)

def render_redirect(page):
    """A page that moved: the old URL keeps working and sends readers (and search engines) to the new place."""
    target = page['redirect']
    return f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="theme-color" content="#0b0d10">
<meta http-equiv="refresh" content="0; url={escape(target)}"><meta name="robots" content="noindex">
<title>{escape(page['title'])}</title><meta name="description" content="{escape(page['description'])}"><link rel="canonical" href="{canonical(target)}">
<link rel="icon" href="assets/favicon.svg" type="image/svg+xml"><link rel="stylesheet" href="style.css"></head><body class="redirect-page">
<main id="main" class="container"><h1>{escape(page['heading'])}</h1><p>{escape(page['lead'])} <a href="{escape(target)}">{escape(page['link_label'])}</a></p></main>
</body></html>
'''

def render(page):
    if 'redirect' in page:
        return render_redirect(page)
    slug = page['slug']
    url = BASE if slug == 'index' else BASE + slug + '.html'
    schema = {'@context': 'https://schema.org', '@type': 'SoftwareApplication', 'name': 'Elite Nexus', 'applicationCategory': 'GameApplication', 'applicationSubCategory': 'Game companion', 'operatingSystem': 'Windows 10, Windows 11, Linux (64-bit), macOS 11 or later', 'softwareVersion': VERSION, 'description': page['description'], 'url': BASE, 'downloadUrl': DOWNLOAD, 'codeRepository': REPO, 'license': REPO + '/blob/main/LICENSE', 'isAccessibleForFree': True, 'offers': {'@type': 'Offer', 'price': '0', 'priceCurrency': 'USD'}, 'featureList': FEATURES, 'keywords': KEYWORDS, 'screenshot': [BASE + 'screenshots/dashboard.png', BASE + 'screenshots/market.png', BASE + 'screenshots/exobiology.png'], 'author': {'@type': 'Person', 'name': 'uruskan', 'url': 'https://github.com/uruskan'}, 'sameAs': [REPO]}
    body = page_body(page)
    faq = faq_schema(body)
    scripts = f'<script type="application/ld+json">{json.dumps(schema)}</script>' + (f'<script type="application/ld+json">{json.dumps(faq)}</script>' if faq else '')
    return f'''<!doctype html>
<html lang="en" id="top"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="theme-color" content="#0b0d10"><meta name="google-site-verification" content="qddrMPdC8qXkPKaDAJsy4fdm2HvshZ6FeSoM-YSG02w">
<title>{escape(page['title'])}</title><meta name="description" content="{escape(page['description'])}"><link rel="canonical" href="{url}">
<meta property="og:type" content="website"><meta property="og:site_name" content="Elite Nexus"><meta property="og:title" content="{escape(page['title'])}"><meta property="og:description" content="{escape(page['description'])}"><meta property="og:url" content="{url}"><meta property="og:image" content="{BASE}assets/social-preview.jpg"><meta property="og:image:width" content="1280"><meta property="og:image:height" content="640"><meta property="og:image:alt" content="Elite Nexus: cockpit customization, trading, exploration, engineering and more in one companion app for Windows, Linux and macOS."><meta name="twitter:card" content="summary_large_image">
<link rel="icon" href="assets/favicon.svg" type="image/svg+xml"><link rel="preload" href="fonts/Inter-Regular.ttf" as="font" type="font/ttf" crossorigin><link rel="stylesheet" href="style.css"><script>document.documentElement.classList.add('js');</script><script src="site.js" defer></script>
{scripts}</head><body class="{'home' if slug == 'index' else 'detail-page'}">
{header(slug)}<main id="main">{body}</main>{footer()}
</body></html>\n'''

def build(destination):
    protected = (SOURCE / 'assets', ROOT / 'docs', ROOT / 'data', ROOT / 'tools', ROOT / '.git')
    if any(destination.is_relative_to(folder.resolve()) for folder in protected):
        raise ValueError('Output must not be inside a source asset, documentation, tools or Git directory.')
    destination.mkdir(parents=True, exist_ok=True)
    pages = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(CONTENT.glob('*.json'))]
    for page in pages:
        if not re.fullmatch(r'[a-z0-9]+(?:-[a-z0-9]+)*', page['slug']):
            raise ValueError(f'Invalid page slug: {page["slug"]!r}')
        (destination / (page['slug'] + '.html')).write_text(render(page), encoding='utf-8')
    if destination.resolve() != SOURCE.resolve():
        for name in ['style.css', 'site.js'] + [p.name for p in SOURCE.glob('*.txt')]:
            shutil.copy2(SOURCE / name, destination / name)
        shutil.copytree(SOURCE / 'assets', destination / 'assets', dirs_exist_ok=True)
    for source, target in [(ROOT / 'docs/screenshots', destination / 'screenshots'), (ROOT / 'data/fonts/inter', destination / 'fonts')]:
        shutil.copytree(source, target, dirs_exist_ok=True)
    shutil.copy2(ROOT / 'docs/social-preview.png', destination / 'social-preview.png')
    shutil.copy2(ROOT / 'docs/USER_GUIDE.md', destination / 'user-guide.md')
    listed = [p for p in pages if 'redirect' not in p]
    urls = ''.join(f'<url><loc>{BASE if p["slug"] == "index" else BASE + p["slug"] + ".html"}</loc></url>\n' for p in listed)
    (destination / 'sitemap.xml').write_text('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' + urls + '</urlset>\n', encoding='utf-8')
    print(f'Built {len(pages)} pages in {destination} ({len(pages) - len(listed)} moved-page redirects)')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=SOURCE)
    build(parser.parse_args().output.resolve())
