"""Cut the promo's pieces out of claudibeat's own renders.

    python -I docs/promo/assets.py <claudibeat.exe> <lang>

Runs `claudibeat --render` for the language and crops the cards the promo
shows (toasts, the sessions window, the board, the dragged card) into
docs/promo/build/<lang>/. Every piece keeps the renders' 2x pixels.
"""
import json
import subprocess
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
BG = (210, 212, 216)


def white_runs(im, x, y0, y1):
    """Vertical runs of card-white pixels in column x."""
    runs, start = [], None
    for y in range(y0, y1):
        on = im.getpixel((x, y))[0] >= 244
        if on and start is None:
            start = y
        if not on and start is not None:
            if y - start > 40:
                runs.append((start, y))
            start = None
    return runs


def white_bbox(im, bottom_cut=96):
    w, h = im.size
    top = im.crop((0, 0, w, h - bottom_cut)).convert("L").point(lambda v: 255 if v >= 244 else 0)
    return top.getbbox()


def dark_box(im, x0, x1, y0, y1):
    """The dragged card: the leftmost column in the window with a card-high
    dark run is its left frame; its top frame runs right from there."""
    dark = lambda c: sum(c) < 200
    for x in range(x0, x1):
        ys = [y for y in range(y0, y1) if dark(im.getpixel((x, y)))]
        if len(ys) > 80 and ys[-1] - ys[0] < 220:
            mid = (ys[0] + ys[-1]) // 2
            # The right frame: the next column dark across the middle.
            right = next(r for r in range(x + 100, x + 900)
                         if all(dark(im.getpixel((r, mid + d))) for d in (-20, 0, 20)))
            while dark(im.getpixel((right + 1, mid))):
                right += 1
            # Top and bottom frames, away from the rounded corners.
            edge = [y for y in range(ys[0] - 20, ys[-1] + 20) if dark(im.getpixel((x + 40, y)))]
            return x, edge[0], right + 1, edge[-1] + 1
    raise SystemExit("no dragged card found")


def main():
    exe, lang = str(Path(sys.argv[1]).resolve()), sys.argv[2]
    raw = HERE / "build" / f"raw-{lang}"
    out = HERE / "build" / lang
    out.mkdir(parents=True, exist_ok=True)
    subprocess.run([exe, "--render", str(raw), "--lang", lang], check=True)
    meta = {}

    toasts = Image.open(raw / "toasts.png").convert("RGB")
    runs = white_runs(toasts, 30, 0, toasts.size[1] - 96)
    (w0, w1), (d0, d1) = runs[0], runs[1]
    x0, _, x1, _ = white_bbox(toasts)
    toasts.crop((x0 - 1, w0 - 1, x1 + 1, w1 + 1)).save(out / "toast-wait.png")
    toasts.crop((x0 - 1, d0 - 1, x1 + 1, d1 + 1)).save(out / "toast-done.png")

    fly = Image.open(raw / "flyout.png").convert("RGB")
    b = white_bbox(fly)
    fly.crop((b[0] - 1, b[1] - 1, b[2] + 1, b[3] + 1)).save(out / "flyout.png")

    board = Image.open(raw / "board.png").convert("RGB")
    drag = Image.open(raw / "board-drag-agent.png").convert("RGB")
    b = white_bbox(board)
    box = (b[0] - 1, b[1] - 1, b[2] + 1, b[3] + 1)
    board.crop(box).save(out / "board.png")
    drag.crop(box).save(out / "board-drag.png")
    # The dragged card: its dark frame between the third and fourth columns.
    w = board.size[0]
    g = dark_box(drag, int(w * 0.5), int(w * 0.70), 430, 800)
    drag.crop(g).save(out / "ghost.png")
    # Where the card was picked up: first card of the first column.
    col = b[0] + 58  # inside the first column's cards, left of their text
    runs = white_runs(board, col, b[1] + 200, b[3] - 100)
    s0, s1 = runs[0]
    row = s0 + 6
    xl = col
    while board.getpixel((xl - 1, row))[0] >= 250:
        xl -= 1
    xr = col
    while board.getpixel((xr + 1, row))[0] >= 250:
        xr += 1
    src = (xl - 4, s0 - 4, xr + 5, s1 + 4)  # with the frame and a little room
    drag.crop(src).save(out / "source-faded.png")
    meta["board"] = {"x": box[0], "y": box[1], "w": box[2] - box[0], "h": box[3] - box[1]}
    meta["ghost"] = {"x": g[0] - box[0], "y": g[1] - box[1], "w": g[2] - g[0], "h": g[3] - g[1]}
    meta["source"] = {"x": src[0] - box[0], "y": src[1] - box[1], "w": src[2] - src[0], "h": src[3] - src[1]}
    for name in ["toast-wait", "toast-done", "flyout"]:
        im = Image.open(out / f"{name}.png")
        meta[name] = {"w": im.size[0], "h": im.size[1]}
    (out / "meta.json").write_text(json.dumps(meta, indent=1))
    print(lang, json.dumps(meta))


if __name__ == "__main__":
    main()
