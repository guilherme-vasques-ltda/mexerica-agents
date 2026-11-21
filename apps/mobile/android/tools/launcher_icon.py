#!/usr/bin/env python3
# Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
"""Regenera os mipmaps de launcher a partir do ícone legado aprovado da Mexerica.

Os ícones não são mais vetores gerados do logo antigo: o pacote de marca em
brand/assets/logo/05-icone-app/ fornece os PNGs (icone-legado-192.png para os
mipmaps, icone-play-store-512.png para a Play Store). O foreground/adaptive,
o ícone monocromático, o ícone de notificação e o mascote do splash são PNGs
conferidos em res/drawable-nodpi/, compostos a partir de 04-mascote e
03-simbolo do mesmo pacote. Rode a partir de apps/android:

    python3 tools/launcher_icon.py /caminho/para/05-icone-app/android
"""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DENSITIES = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}


def main() -> None:
    src_dir = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else None
    if src_dir is None:
        sys.exit(__doc__)
    icon = src_dir / "icone-legado-192.png"
    res = ROOT / "app/src/main/res"
    for density, size in DENSITIES.items():
        out = res / f"mipmap-{density}" / "ic_launcher.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["sips", "-z", str(size), str(size), str(icon), "--out", str(out)], check=True,
                       stdout=subprocess.DEVNULL)
    print(f"{len(DENSITIES)} mipmaps de {icon}")


if __name__ == "__main__":
    main()
