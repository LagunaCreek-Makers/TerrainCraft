"""Build the bundled engraving icon library from upstream icon packages.

Copies a curated subset of Tabler Icons (MIT, outline style) and Maki (CC0) into
assets/icon_library, keeps each set's license file next to its icons, and writes
catalog.json (set provenance plus per-icon metadata) and the Rust table that embeds them.

Usage (after extracting the npm tarballs):
    python tools/build_icon_library.py --tabler path/to/@tabler/icons/package \
        --maki path/to/@mapbox/maki/package
"""
import argparse
import json
import re
import shutil
from pathlib import Path

# Tabler categories that make sense on a terrain engraving. Brands, UI glyphs, letters,
# numbers and device icons are left out.
TABLER_CATEGORIES = {
    'Map', 'Buildings', 'Nature', 'Sport', 'Vehicles', 'Weather', 'Animals', 'Food',
    'Symbols',
}
# Status and UI modifiers: `map-pin-heart`, `car-off`, `building-plus` and so on.
TABLER_MODIFIER = re.compile(
    r'-(off|plus|minus|check|x|search|bolt|cancel|cog|code|dollar|down|up|exclamation|heart'
    r'|pause|pin|question|share|star|filled|discount|bitcoin|euro|pound|lock|hexagon|square'
    r'|circle|spark|edit|stats|left|right|alt|\d+)$')
TABLER_EXCLUDE_PREFIX = ('badge', 'zoom', 'car-fan', 'home-', 'world-', 'eye-', 'michelin-')
# Trademarks, legal marks and interface glyphs.
TABLER_EXCLUDE = {
    'ce', 'copyright', 'copyleft', 'registered', 'trademark', 'servicemark', 'tir', 'command',
    'option', 'pokeball', 'olympics', 'live-view', 'location-broken', 'alien', 'automatic-gearbox',
    'car-door', 'car-lifter', 'car-suspension', 'car-turbine', 'door-hanger', 'checkup-list',
    'plunger', 'razor', 'razor-electric', 'wiper', 'wiper-wash', 'tic-tac', 'meeple',
}
# Maki's plain geometric shapes duplicate nothing useful.
MAKI_EXCLUDE = re.compile(r'^(arrow|circle|square|triangle|star|marker|diamond)(-stroked)?$')

TABLER_BOX = re.compile(r'\s*<path stroke="none" d="M0 0h24v24H0z" fill="none"\s*/>')


def tabler_svg(text):
    """Drop the invisible 24x24 bounding-box path and the CSS class."""
    text = TABLER_BOX.sub('', text)
    return re.sub(r'\s*class="[^"]*"', '', text)


def maki_svg(text):
    text = re.sub(r'<\?xml[^>]*\?>\s*', '', text)
    return re.sub(r'\s+id="[^"]*"', '', text)


def title(name):
    return name.replace('-JP', ' (Japan)').replace('-', ' ').strip().capitalize()


def build(tabler, maki, output, rust):
    output.mkdir(parents=True, exist_ok=True)
    for d in ('tabler', 'maki'):
        shutil.rmtree(output / d, ignore_errors=True)
        (output / d).mkdir()
    sets, icons = [], []

    meta = json.loads((tabler / 'icons.json').read_text(encoding='utf-8'))
    version = json.loads((tabler / 'package.json').read_text(encoding='utf-8'))['version']
    shutil.copyfile(tabler / 'LICENSE', output / 'tabler' / 'LICENSE')
    sets.append(dict(name='tabler', title='Tabler Icons', version=version, license='MIT',
                     license_file='tabler/LICENSE', source='https://tabler.io/icons',
                     package=f'@tabler/icons@{version}'))
    for name, m in sorted(meta.items()):
        if ('outline' not in m.get('styles', {}) or m.get('category') not in TABLER_CATEGORIES
                or TABLER_MODIFIER.search(name) or name.startswith(TABLER_EXCLUDE_PREFIX)
                or name in TABLER_EXCLUDE):
            continue
        src = tabler / 'icons' / 'outline' / f'{name}.svg'
        if not src.is_file():
            continue
        (output / 'tabler' / f'{name}.svg').write_text(
            tabler_svg(src.read_text(encoding='utf-8')), encoding='utf-8', newline='\n')
        icons.append(dict(id=f'tabler:{name}', title=title(name), set='tabler',
                          category=m['category'], tags=[str(t) for t in m.get('tags', [])]))

    version = json.loads((maki / 'package.json').read_text(encoding='utf-8'))['version']
    shutil.copyfile(maki / 'LICENSE.txt', output / 'maki' / 'LICENSE.txt')
    sets.append(dict(name='maki', title='Maki', version=version, license='CC0-1.0',
                     license_file='maki/LICENSE.txt', source='https://labs.mapbox.com/maki-icons/',
                     package=f'@mapbox/maki@{version}'))
    for src in sorted((maki / 'icons').glob('*.svg')):
        name = src.stem
        if MAKI_EXCLUDE.match(name):
            continue
        (output / 'maki' / f'{name}.svg').write_text(
            maki_svg(src.read_text(encoding='utf-8')), encoding='utf-8', newline='\n')
        icons.append(dict(id=f'maki:{name}', title=title(name), set='maki',
                          category='Points of interest', tags=name.lower().split('-')))

    (output / 'catalog.json').write_text(
        json.dumps(dict(sets=sets, icons=icons), indent=1, ensure_ascii=False) + '\n',
        encoding='utf-8', newline='\n')
    rows = ''.join(
        f'    ("{i["id"]}", include_str!("../../../assets/icon_library/{i["set"]}/'
        f'{i["id"].split(":", 1)[1]}.svg")),\n' for i in icons)
    rust.write_text(
        '// Generated by tools/build_icon_library.py; do not edit.\n'
        '#[rustfmt::skip]\n'
        'pub(crate) static SVGS: &[(&str, &str)] = &[\n' + rows + '];\n',
        encoding='utf-8', newline='\n')
    return icons


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument('--tabler', type=Path, required=True, help='extracted @tabler/icons package')
    p.add_argument('--maki', type=Path, required=True, help='extracted @mapbox/maki package')
    p.add_argument('--output', type=Path, default=Path('assets/icon_library'))
    p.add_argument('--rust', type=Path,
                   default=Path('crates/terraincraft-core/src/icon_library_data.rs'))
    a = p.parse_args()
    icons = build(a.tabler, a.maki, a.output, a.rust)
    print(f'{len(icons)} icons written to {a.output}')


if __name__ == '__main__':
    main()
