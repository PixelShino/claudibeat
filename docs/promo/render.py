"""Render the promo to MP4 and GIF.

    python -I docs/promo/render.py en [--fps 30] [--only 6.5,9.4]

Needs docs/promo/build/<lang> from assets.py, Playwright's Chromium and
imageio-ffmpeg. Frames come from scene.html's render(t), one screenshot each,
in a headless browser: nothing touches the desktop.
"""
import argparse
import functools
import http.server
import shutil
import subprocess
import threading
from pathlib import Path

import imageio_ffmpeg
from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent


def serve():
    class Quiet(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass

    handler = functools.partial(Quiet, directory=str(HERE))
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("lang")
    ap.add_argument("--fps", type=int, default=30)
    ap.add_argument("--only", help="comma-separated times: write just these stills")
    ap.add_argument("--gif-width", type=int, default=800)
    ap.add_argument("--gif-fps", type=int, default=15)
    ap.add_argument("--gif-only", action="store_true", help="remake the GIF from the MP4")
    a = ap.parse_args()

    out = HERE / "out"
    out.mkdir(exist_ok=True)
    if a.gif_only:
        return make_gif(a, out)
    httpd = serve()
    url = f"http://127.0.0.1:{httpd.server_address[1]}/scene.html?lang={a.lang}"
    with sync_playwright() as p:
        browser = p.chromium.launch(args=["--disable-lcd-text"])  # grayscale text, as claudibeat draws it
        page = browser.new_page(viewport={"width": 1280, "height": 720}, device_scale_factor=1)
        page.goto(url)
        page.wait_for_function("window.READY === true")
        if a.only:
            for t in a.only.split(","):
                page.evaluate(f"render({float(t)})")
                page.screenshot(path=str(out / f"still-{a.lang}-{float(t):05.2f}.png"))
            browser.close()
            return
        frames = out / f"frames-{a.lang}"
        shutil.rmtree(frames, ignore_errors=True)
        frames.mkdir()
        total = page.evaluate("window.DURATION")
        n = int(total * a.fps)
        for i in range(n):
            page.evaluate(f"render({i / a.fps})")
            page.screenshot(path=str(frames / f"{i:05d}.png"))
        browser.close()

    ff = imageio_ffmpeg.get_ffmpeg_exe()
    mp4 = out / f"claudibeat-{a.lang}.mp4"
    subprocess.run([ff, "-y", "-loglevel", "error", "-framerate", str(a.fps), "-i", str(frames / "%05d.png"),
                    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", "-preset", "slow", "-movflags", "+faststart", str(mp4)], check=True)
    print(mp4, round(mp4.stat().st_size / 1e6, 2), "MB")
    make_gif(a, out)


def make_gif(a, out):
    ff = imageio_ffmpeg.get_ffmpeg_exe()
    mp4 = out / f"claudibeat-{a.lang}.mp4"
    gif = out / f"claudibeat-{a.lang}.gif"
    vf = (f"fps={a.gif_fps},scale={a.gif_width}:-1:flags=lanczos,split[s0][s1];"
          "[s0]palettegen=max_colors=96:stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle")
    subprocess.run([ff, "-y", "-loglevel", "error", "-i", str(mp4), "-vf", vf, "-loop", "0", str(gif)], check=True)
    print(gif, round(gif.stat().st_size / 1e6, 2), "MB")


if __name__ == "__main__":
    main()
