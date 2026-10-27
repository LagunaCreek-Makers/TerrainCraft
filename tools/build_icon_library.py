"""Build the bundled engraving icon library from upstream icon packages.

Copies curated subsets of Tabler Icons (MIT, outline style), Maki (CC0), Temaki (CC0)
and the National Park Service map symbols (BSD) into assets/icon_library, keeps each
set's license file next to its icons, adds TerrainCraft's own landmark icons listed in
assets/icon_library/landmarks/manifest.json, and writes catalog.json (set provenance
plus per-icon metadata) and the Rust table that embeds them.

Usage (after extracting the npm tarballs):
    python tools/build_icon_library.py --tabler path/to/@tabler/icons/package \
        --maki path/to/@mapbox/maki/package --temaki path/to/@rapideditor/temaki/package \
        --nps path/to/npmap-symbol-library/package
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

# Temaki groups worth engraving; icons also in a utility or street-furniture group are left out.
TEMAKI_GROUPS = {
    'sports', 'water', 'buildings', 'plants', 'landforms', 'animals', 'snow', 'camping',
    'aerialways', 'religion', 'arts', 'railways',
}
TEMAKI_EXCLUDE_GROUPS = {
    'highways', 'crossings', 'utilities', 'power', 'vending', 'barriers', 'security', 'telecom',
    'tools', 'home', 'bags', 'clothes', 'shapes',
}

# An empty placeholder and a plain dot.
NPS_EXCLUDE = {'transparent', 'dot'}

TABLER_BOX = re.compile(r'\s*<path stroke="none" d="M0 0h24v24H0z" fill="none"\s*/>')


def tabler_svg(text):
    """Drop the invisible 24x24 bounding-box path and the CSS class."""
    text = TABLER_BOX.sub('', text)
    return re.sub(r'\s*class="[^"]*"', '', text)


def maki_svg(text):
    text = re.sub(r'<\?xml[^>]*\?>\s*', '', text)
    return re.sub(r'\s+id="[^"]*"', '', text)


def nps_svg(text):
    """Illustrator exports: drop the prolog, comments, ids, inline styles and colors."""
    text = re.sub(r'<\?xml[^>]*\?>\s*|<!--.*?-->\s*', '', text, flags=re.S)
    text = re.sub(r'\s+(id|style|xml:space|x|y|enable-background)="[^"]*"', '', text)
    return re.sub(r'\s+fill="#[0-9A-Fa-f]{3,6}"', '', text)


def title(name):
    return name.replace('-JP', ' (Japan)').replace('-', ' ').replace('_', ' ').strip().capitalize()


def package_version(path):
    return json.loads((path / 'package.json').read_text(encoding='utf-8'))['version']


def landmark_icons(output):
    """TerrainCraft's own landmark icons, described by landmarks/manifest.json."""
    manifest = output / 'landmarks' / 'manifest.json'
    if not manifest.is_file():
        return None, []
    m = json.loads(manifest.read_text(encoding='utf-8'))
    icons = []
    for i in m['icons']:
        if not (output / 'landmarks' / f'{i["name"]}.svg').is_file():
            raise SystemExit(f'landmarks/{i["name"]}.svg is listed but missing')
        icon = dict(id=f'landmarks:{i["name"]}', title=i['title'], set='landmarks',
                    category=i['category'], tags=i.get('tags', []))
        for k in ('method', 'license', 'attribution', 'sources', 'review'):
            if k in i:
                icon[k] = i[k]
        icons.append(icon)
    return m['set'], icons


def build(tabler, maki, output, rust, temaki=None, nps=None):
    output.mkdir(parents=True, exist_ok=True)
    for d, pkg in (('tabler', tabler), ('maki', maki), ('temaki', temaki), ('nps', nps)):
        if pkg is not None:
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

    if temaki is not None:
        version = package_version(temaki)
        shutil.copyfile(temaki / 'LICENSE.md', output / 'temaki' / 'LICENSE.md')
        sets.append(dict(name='temaki', title='Temaki', version=version, license='CC0-1.0',
                         license_file='temaki/LICENSE.md',
                         source='https://github.com/rapideditor/temaki',
                         package=f'@rapideditor/temaki@{version}'))
        groups = json.loads((temaki / 'data' / 'icons.json').read_text(encoding='utf-8'))
        for src in sorted((temaki / 'icons').glob('*.svg')):
            name = src.stem
            g = groups.get(name, {}).get('groups', [])
            keep = [x for x in g if x in TEMAKI_GROUPS]
            if not keep or TEMAKI_EXCLUDE_GROUPS.intersection(g) or name == 'temaki':
                continue
            (output / 'temaki' / f'{name}.svg').write_text(
                maki_svg(src.read_text(encoding='utf-8')), encoding='utf-8', newline='\n')
            icons.append(dict(id=f'temaki:{name}', title=title(name), set='temaki',
                              category=keep[0].capitalize(), tags=g + name.split('_')))

    if nps is not None:
        version = package_version(nps)
        shutil.copyfile(nps / 'LICENSE.txt', output / 'nps' / 'LICENSE.txt')
        sets.append(dict(name='nps', title='National Park Service map symbols', version=version,
                         license='BSD-3-Clause', license_file='nps/LICENSE.txt',
                         source='https://github.com/nationalparkservice/symbol-library',
                         package=f'npmap-symbol-library@{version}'))
        for src in sorted((nps / 'src' / 'standalone').glob('*-black-22.svg')):
            name = src.name[:-len('-black-22.svg')]
            if name in NPS_EXCLUDE:
                continue
            (output / 'nps' / f'{name}.svg').write_text(
                nps_svg(src.read_text(encoding='utf-8')), encoding='utf-8', newline='\n')
            icons.append(dict(id=f'nps:{name}', title=title(name), set='nps',
                              category='Park services', tags=name.split('-')))

    landmark_set, landmarks = landmark_icons(output)
    if landmark_set:
        sets.append(landmark_set)
        icons.extend(landmarks)

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
    p.add_argument('--temaki', type=Path, help='extracted @rapideditor/temaki package')
    p.add_argument('--nps', type=Path, help='extracted npmap-symbol-library package')
    p.add_argument('--output', type=Path, default=Path('assets/icon_library'))
    p.add_argument('--rust', type=Path,
                   default=Path('crates/terraincraft-core/src/icon_library_data.rs'))
    a = p.parse_args()
    icons = build(a.tabler, a.maki, a.output, a.rust, a.temaki, a.nps)
    print(f'{len(icons)} icons written to {a.output}')


if __name__ == '__main__':
    main()
