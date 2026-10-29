#!/usr/bin/env python3
"""Builds the VS Code extension in editors/vscode into a .vsix file.

    package_vscode.py [--out FILE]

A .vsix is a zip archive with a manifest; this script writes it with the
standard library only, with fixed timestamps, so no Node tooling or network
access is needed. Install the result with
`code --install-extension weave-syntax-<version>.vsix`.

The package is for local installation: it carries the placeholder publisher
`weave-local` and `"private": true`. Publishing to a marketplace needs a
publisher account of the maintainer and is not done here.
"""
import argparse
import json
import sys
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parent.parent
EXT = ROOT / "editors" / "vscode"
FIXED_DATE = (2024, 1, 1, 0, 0, 0)
FILES = [
    "package.json",
    "README.md",
    "LICENSE",
    "language-configuration.json",
    "syntaxes/weave.tmLanguage.json",
]
CONTENT_TYPES = {
    ".json": "application/json",
    ".md": "text/markdown",
    ".vsixmanifest": "text/xml",
}


def manifest_xml(pkg: dict) -> str:
    engine = pkg["engines"]["vscode"]
    tags = ",".join(pkg.get("keywords", []))
    categories = ",".join(pkg.get("categories", []))
    return f"""<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011" xmlns:d="http://schemas.microsoft.com/developer/vsx-schema-design/2011">
  <Metadata>
    <Identity Language="en-US" Id="{escape(pkg['name'])}" Version="{escape(pkg['version'])}" Publisher="{escape(pkg['publisher'])}" />
    <DisplayName>{escape(pkg['displayName'])}</DisplayName>
    <Description xml:space="preserve">{escape(pkg['description'])}</Description>
    <Tags>{escape(tags)}</Tags>
    <Categories>{escape(categories)}</Categories>
    <GalleryFlags>Public</GalleryFlags>
    <Properties>
      <Property Id="Microsoft.VisualStudio.Code.Engine" Value="{escape(engine)}" />
      <Property Id="Microsoft.VisualStudio.Code.ExtensionDependencies" Value="" />
      <Property Id="Microsoft.VisualStudio.Code.ExtensionPack" Value="" />
      <Property Id="Microsoft.VisualStudio.Code.ExtensionKind" Value="workspace,ui" />
      <Property Id="Microsoft.VisualStudio.Code.LocalizedLanguages" Value="" />
      <Property Id="Microsoft.VisualStudio.Services.Links.Source" Value="{escape(pkg['repository']['url'])}" />
      <Property Id="Microsoft.VisualStudio.Services.Links.Support" Value="{escape(pkg['bugs']['url'])}" />
    </Properties>
    <License>extension/LICENSE</License>
  </Metadata>
  <Installation>
    <InstallationTarget Id="Microsoft.VisualStudio.Code" />
  </Installation>
  <Dependencies />
  <Assets>
    <Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" Addressable="true" />
    <Asset Type="Microsoft.VisualStudio.Services.Content.Details" Path="extension/README.md" Addressable="true" />
    <Asset Type="Microsoft.VisualStudio.Services.Content.License" Path="extension/LICENSE" Addressable="true" />
  </Assets>
</PackageManifest>
"""


def content_types_xml() -> str:
    defaults = "".join(
        f'<Default Extension="{ext}" ContentType="{ctype}"/>' for ext, ctype in sorted(CONTENT_TYPES.items())
    )
    return (
        '<?xml version="1.0" encoding="utf-8"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        f"{defaults}"
        '<Override PartName="/extension/LICENSE" ContentType="text/plain"/>'
        "</Types>"
    )


def build(out: Path) -> None:
    pkg = json.loads((EXT / "package.json").read_text(encoding="utf-8"))
    entries = [
        ("[Content_Types].xml", content_types_xml().encode("utf-8")),
        ("extension.vsixmanifest", manifest_xml(pkg).encode("utf-8")),
    ]
    for name in FILES:
        data = (EXT / name).read_bytes().replace(b"\r\n", b"\n")
        entries.append((f"extension/{name}", data))
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, data)
    print(f"wrote {out} ({out.stat().st_size} bytes, {len(entries)} files)")


def main() -> int:
    pkg = json.loads((EXT / "package.json").read_text(encoding="utf-8"))
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", default=f"{pkg['name']}-{pkg['version']}.vsix")
    args = parser.parse_args()
    build(Path(args.out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
