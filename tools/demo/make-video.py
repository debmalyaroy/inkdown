"""Builds the demo video and the README animation from the captured screenshots.

    pwsh tools/demo/capture.ps1      # writes docs/media/shots/*.png
    python tools/demo/make-video.py  # writes docs/media/inkdown-demo.mp4 and .gif

Needs Pillow and imageio-ffmpeg (pip install pillow imageio-ffmpeg).
"""
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
SHOTS = ROOT / "docs" / "media" / "shots"
ANDROID = ROOT / "docs" / "media" / "android"
OUT = ROOT / "docs" / "media"
FONT = ROOT / "native" / "fonts" / "Inter.ttf"
ICON = ROOT / "assets" / "icon.png"

W, BAR = 1280, 96                       # video frame: screenshot plus a caption bar
_first = sorted(SHOTS.glob("*.png"))
_size = Image.open(_first[0]).size if _first else (1600, 1050)
SHOT_H = int(W * _size[1] / _size[0]) // 2 * 2   # keep the screenshots' own aspect ratio
H = SHOT_H + BAR
FPS = 20
HOLD, FADE = 3.4, 0.45                  # seconds per scene, seconds of cross-fade
BG, FG, MUTED, ACCENT = (17, 19, 26), (236, 238, 245), (150, 156, 175), (124, 120, 255)

# (file, title, detail)
SCENES = [
    ("01-read.png", "Read mode", "Markdown as a reader sees it: front matter, callouts, task lists, footnotes"),
    ("02-math-diagrams.png", "Math and diagrams", "LaTeX math and Mermaid diagrams are drawn natively, with no web view"),
    ("03-diagrams.png", "More diagram types", "Pie charts and Graphviz graphs from fenced code blocks"),
    ("04-charts.png", "Charts and images", "Chart blocks from JSON, inline images with captions, click to zoom"),
    ("05-write.png", "Write mode", "Word-style editing with a formatting ribbon; the file stays plain Markdown"),
    ("06-split.png", "Split mode", "Source on the left, live preview on the right, scrolling together"),
    ("07-code.png", "Code mode", "A Sublime-style text editor: minimap, folding, snippets, Markdown toolbar"),
    ("08-palette.png", "Command palette", "Ctrl+Shift+P finds every command, with its shortcut"),
    ("09-export.png", "Export", "HTML, Word, RTF and PDF, or copy as rich text for email and docs"),
    ("10-theme-light.png", "Nine themes", "Light"),
    ("11-theme-sepia.png", "Nine themes", "Sepia"),
    ("12-theme-nord.png", "Nine themes", "Nord"),
    ("13-theme-dracula.png", "Nine themes", "Dracula"),
    ("14-project.png", "Open a folder", "File tree and outline in the sidebar, syntax colouring for common languages"),
    ("15-multicursor.png", "Multiple cursors", "Ctrl+D selects the next occurrence; type once, edit them all"),
    ("16-goto.png", "Goto Anything", "Ctrl+P jumps to any file, :line, @symbol or #word"),
    ("17-json.png", "JSON, YAML and TOML", "A collapsible tree, validation, formatting and conversion between formats"),
    ("18-csv.png", "CSV as a table", "Tabular data next to its source; convert to a Markdown table in one step"),
    ("19-markdown-split.png", "Notes next to code", "Project documents render with their diagrams while you edit"),
    ("20-two-panes.png", "Two panes", "Keep a document and a source file side by side"),
    ("21-find-in-files.png", "Find in Files", "Search and replace across the folder, with regular expressions"),
    ("22-designer.png", "Diagram designer", "Draw with blocks and connectors; saved as SVG that reopens for editing"),
]


def font(size, weight=500):
    f = ImageFont.truetype(str(FONT), size)
    try:
        axes = f.get_variation_axes()
        values = [weight if a.get("name") in (b"Weight", "Weight") or a.get("tag") == "wght" else a["default"] for a in axes]
        f.set_variation_by_axes(values)
    except Exception:
        pass
    return f


F_TITLE, F_BODY, F_HERO, F_SUB, F_STEP = font(30, 700), font(20, 450), font(64, 750), font(26, 450), font(16, 600)


def scene(path, title, detail, step, total):
    frame = Image.new("RGB", (W, H), BG)
    shot = Image.open(path).convert("RGB").resize((W, SHOT_H), Image.LANCZOS)
    frame.paste(shot, (0, 0))
    d = ImageDraw.Draw(frame)
    d.rectangle([0, SHOT_H, W, H], fill=BG)
    d.rectangle([0, SHOT_H, W, SHOT_H + 2], fill=(40, 43, 56))
    d.rectangle([0, SHOT_H, int(W * step / total), SHOT_H + 2], fill=ACCENT)
    d.text((36, SHOT_H + 18), title, font=F_TITLE, fill=FG)
    d.text((36, SHOT_H + 58), detail, font=F_BODY, fill=MUTED)
    label = f"{step} / {total}"
    d.text((W - 36 - d.textlength(label, font=F_STEP), SHOT_H + 26), label, font=F_STEP, fill=MUTED)
    return frame


def card(title, lines, with_icon=True):
    frame = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(frame)
    y = H // 2 - 220
    if with_icon and ICON.exists():
        icon = Image.open(ICON).convert("RGBA").resize((144, 144), Image.LANCZOS)
        frame.paste(icon, ((W - 144) // 2, H // 2 - 320), icon)
        y = H // 2 - 140
    d.text(((W - d.textlength(title, font=F_HERO)) / 2, y), title, font=F_HERO, fill=FG)
    y += 110
    for line in lines:
        d.text(((W - d.textlength(line, font=F_SUB)) / 2, y), line, font=F_SUB, fill=MUTED)
        y += 44
    return frame


def phones():
    """Three phone screenshots side by side."""
    frame = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(frame)
    files = sorted(ANDROID.glob("*.png"))[:4]
    ph = SHOT_H - 80
    pw = int(ph * 1080 / 2400)
    gap = (W - len(files) * pw) // (len(files) + 1)
    for i, f in enumerate(files):
        im = Image.open(f).convert("RGB").resize((pw, ph), Image.LANCZOS)
        x = gap + i * (pw + gap)
        d.rounded_rectangle([x - 6, 34, x + pw + 6, 46 + ph], radius=22, fill=(48, 51, 66))
        frame.paste(im, (x, 40))
    d.rectangle([0, SHOT_H, W, SHOT_H + 2], fill=(40, 43, 56))
    d.text((36, SHOT_H + 18), "Android", font=F_TITLE, fill=FG)
    d.text((36, SHOT_H + 58), "The same Rust code on a phone: read, write, Open with Inkdown, on-screen keyboard", font=F_BODY, fill=MUTED)
    return frame


def main():
    missing = [s[0] for s in SCENES if not (SHOTS / s[0]).exists()]
    if missing:
        sys.exit(f"Missing screenshots (run tools/demo/capture.ps1 first): {missing}")
    total = len(SCENES) + 1
    frames = [card("Inkdown", ["A fast, native editor for Markdown, code, data and diagrams", "Pure Rust. No web view. One small executable."])]
    frames += [scene(SHOTS / f, t, dsc, i + 1, total) for i, (f, t, dsc) in enumerate(SCENES)]
    if any(ANDROID.glob("*.png")):
        frames.append(phones())
    frames.append(card("Get Inkdown", ["github.com/debmalyaroy/inkdown/releases", "Windows installer  ·  Android APK  ·  macOS and Linux previews"]))

    # --- MP4 (H.264) through ffmpeg, frames piped as raw RGB
    import imageio_ffmpeg

    mp4 = OUT / "inkdown-demo.mp4"
    cmd = [imageio_ffmpeg.get_ffmpeg_exe(), "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
           "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(mp4)]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE)
    hold, fade = int(HOLD * FPS), int(FADE * FPS)
    for i, frame in enumerate(frames):
        data = frame.tobytes()
        short = i > 0 and SCENES[min(i, len(SCENES)) - 1][1] == "Nine themes" and i <= len(SCENES)
        for _ in range(hold // 2 if short else hold):
            proc.stdin.write(data)
        if i + 1 < len(frames):
            for k in range(1, fade + 1):
                proc.stdin.write(Image.blend(frame, frames[i + 1], k / (fade + 1)).tobytes())
    proc.stdin.close()
    if proc.wait() != 0:
        sys.exit("ffmpeg failed")

    # --- GIF for the README (GitHub does not play repository videos inline)
    gw = 880
    gh = int(H * gw / W)
    small = [f.resize((gw, gh), Image.LANCZOS).quantize(colors=128, method=Image.MEDIANCUT, dither=Image.NONE) for f in frames]
    durations = [1400 if 0 < i <= len(SCENES) and SCENES[i - 1][1] == "Nine themes" else 2600 for i in range(len(frames))]
    gif = OUT / "inkdown-demo.gif"
    small[0].save(gif, save_all=True, append_images=small[1:], duration=durations, loop=0, optimize=True)

    for f in (mp4, gif):
        print(f"{f.relative_to(ROOT)}  {f.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
