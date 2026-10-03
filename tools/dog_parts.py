#!/usr/bin/env python3
"""Make the dog's parts (assets/companion/dog-parts.png and .json) from the
MapleSyrup logo: the chow chow in the syrup captain's hat and cape is cut out
of logos/final_logo.png with an illustration segmentation model (IS-Net
anime, the one rembg uses: downloaded once, 170 MB), the grass and the rest
of the picture are cleared away, and the dog is split into puppet parts: the
tail (with a hidden base behind the cape, so it never comes loose as it
wags), the body with the cape, the two front paws, the head with its eyes
and open mouth taken out (filled in with fur), the eyes, the open mouth, and
a closed smile drawn along the top of it. The layout goes into the JSON and
into src/phone/dog.js.

    pip install numpy pillow opencv-python-headless onnxruntime
    python3 tools/dog_parts.py [logos/final_logo.png]
"""
import json, os, sys, urllib.request
import numpy as np, cv2, onnxruntime as ort
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "companion")
LOGO = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "logos", "final_logo.png")
# Where the dog is in the logo.
BOX = (248, 195, 552, 690)
MODEL_URL = "https://github.com/danielgatis/rembg/releases/download/v0.0.0/isnet-anime.onnx"
MODEL = os.path.join(os.path.expanduser("~"), ".cache", "maplesyrup", "isnet-anime.onnx")

# ---- cut the dog out ----
if not os.path.exists(MODEL):
    os.makedirs(os.path.dirname(MODEL), exist_ok=True)
    print("downloading", MODEL_URL)
    urllib.request.urlretrieve(MODEL_URL, MODEL)
img = Image.open(LOGO).convert("RGB").crop(BOX)
sess = ort.InferenceSession(MODEL, providers=["CPUExecutionProvider"])
a = np.array(img.resize((1024, 1024), Image.Resampling.LANCZOS)).astype(np.float32)
a = (a / a.max() - np.array([0.485, 0.456, 0.406])).transpose(2, 0, 1)[None].astype(np.float32)
pred = sess.run(None, {sess.get_inputs()[0].name: a})[0][0, 0]
pred = (pred - pred.min()) / (pred.max() - pred.min())
mask = np.array(Image.fromarray((pred * 255).astype("uint8"), "L").resize(img.size, Image.Resampling.LANCZOS)).astype(np.float32)
rgb0 = np.array(img)
# The grass (greenish), what's below the paws, and anything not joined to the dog.
hsv0 = cv2.cvtColor(rgb0, cv2.COLOR_RGB2HSV)
h0, s0, v0 = hsv0[..., 0].astype(int) * 2, hsv0[..., 1] / 255.0, hsv0[..., 2] / 255.0
mask[(h0 >= 58) & (h0 <= 170) & (s0 > 0.25) & (v0 > 0.2)] = 0
mask[488:, :] = 0
n0, lab0, st0, _ = cv2.connectedComponentsWithStats((mask > 40).astype(np.uint8), 8)
big0 = 1 + np.argmax(st0[1:, cv2.CC_STAT_AREA])
mask[~(cv2.dilate((lab0 == big0).astype(np.uint8), np.ones((3, 3), np.uint8)) > 0)] = 0
full = Image.fromarray(np.dstack([rgb0, mask.astype(np.uint8)]), "RGBA")

src = np.array(full).astype(np.float32)
H, W = src.shape[:2]
rgb = src[..., :3].astype(np.uint8)
alpha = src[..., 3] / 255.0
hsv = cv2.cvtColor(rgb, cv2.COLOR_RGB2HSV)
hue, sat, val = hsv[..., 0].astype(int) * 2, hsv[..., 1] / 255.0, hsv[..., 2] / 255.0
yy, xx = np.mgrid[0:H, 0:W]

# Leftover grass at the bottom left.
grass = (xx < 40) & (yy > 395) & (hue >= 50) & (hue <= 170) & (sat > 0.2)
alpha[grass] = 0

def ramp(v, a, b):
    return np.clip((v - a) / (b - a), 0, 1)

# ---- the tail: right of the cape's edge, with a base reaching in behind
# the cape (a mirror of itself), so it never comes loose as it wags ----
p1, p2 = np.array([254.0, 324.0]), np.array([289.0, 410.0])
d = p2 - p1; dl = np.hypot(*d)
side = ((d[0]) * (yy - p1[1]) - (d[1]) * (xx - p1[0])) / dl  # px; < 0: right of the line
band = (yy >= 324) & (yy <= 414) & (xx >= 236)
tail_m = band & (side < 1.5)
# Only the tail itself: its biggest piece (not a bit of the cape's trim).
n_, lab_, st_, _ = cv2.connectedComponentsWithStats(((alpha > 0.3) & tail_m & (side < -1.5)).astype(np.uint8), 8)
main_ = 1 + np.argmax(st_[1:, cv2.CC_STAT_AREA])
keep_ = cv2.dilate((lab_ == main_).astype(np.uint8), np.ones((5, 5), np.uint8)) > 0
tail_m &= keep_ | (side >= -1.5)
tail_a = alpha * tail_m
tail_rgb = rgb.copy()
# Behind the cape: the tail reflected across the edge.
nx, ny = -d[1] / dl, d[0] / dl  # the line's normal (pointing left, behind the cape)
behind = band & (side >= 1.5) & (side < 16)
ry_ = np.clip(np.round(yy - 2 * side * ny), 0, H - 1).astype(int)
rx_ = np.clip(np.round(xx - 2 * side * nx), 0, W - 1).astype(int)
tail_rgb[behind] = rgb[ry_[behind], rx_[behind]]
core_ = cv2.dilate((lab_ == main_).astype(np.uint8), np.ones((3, 3), np.uint8)) > 0
tail_a = np.where(behind, alpha[ry_, rx_] * core_[ry_, rx_], tail_a)
# (Not the cape's own edge at the top.)
tail_a[(yy < 335) & (xx < 259)] = 0
# ---- the paws ----
paw_boxes = {"pawL": (62, 120), "pawR": (158, 214)}
paw_top, paw_soft = 430, 12
paws = {}
for name, (x0, x1) in paw_boxes.items():
    m = (xx >= x0) & (xx < x1) & (yy >= paw_top)
    paws[name] = alpha * m * ramp(yy, paw_top, paw_top + paw_soft)
# ---- the head: above the neck ----
neck = 318
head_a = alpha * (1 - ramp(yy, neck - 6, neck + 6))
# ---- the body: the rest; behind the head only a plain neck of fur ----
body_a = alpha.copy()
body_a *= 1 - (band & (side < -1.5))
for name, (x0, x1) in paw_boxes.items():
    under = (xx >= x0 + 3) & (xx < x1 - 3) & (yy >= 448)
    body_a[under] = 0
body_rgb = rgb.copy()
# Above the neck: fur colour, no face (so nothing doubles when the head moves).
fur = np.median(rgb[(yy > 318) & (yy < 334) & (xx > 125) & (xx < 175) & (alpha > 0.9)], axis=0)
shade = fur * 0.93
top = yy < 306
grad = ramp(yy, 200, 306)[..., None]
body_rgb[top] = (fur * grad[top] + shade * (1 - grad[top])).astype(np.uint8)
ell = ((xx - 150) / 128.0) ** 2 + ((yy - 300) / 70.0) ** 2
neck_a = np.clip((1 - ell) * 6, 0, 1)
body_a = np.where(top, np.minimum(alpha, neck_a), body_a * np.where(yy < 312, np.maximum(neck_a, ramp(yy, 306, 312)), 1))

# ---- the face: eyes and the open mouth taken out of the head ----
def feature(box, dark_val, extra=None):
    x0, y0, x1, y1 = box
    m = np.zeros((H, W), np.uint8)
    region = (xx >= x0) & (xx < x1) & (yy >= y0) & (yy < y1)
    sel = region & (val < dark_val)
    if extra is not None:
        sel |= region & extra
    m[sel] = 255
    # Fill what's enclosed (the shine in the eyes, the tongue).
    m = cv2.morphologyEx(m, cv2.MORPH_CLOSE, np.ones((5, 5), np.uint8))
    filled = m.copy()
    ff = np.zeros((H + 2, W + 2), np.uint8)
    cv2.floodFill(filled, ff, (0, 0), 255)
    holes = cv2.bitwise_not(filled)
    m = m | holes
    m = cv2.dilate(m, np.ones((3, 3), np.uint8))
    return m

eyeL_m = feature((84, 186, 122, 214), 0.55)
eyeR_m = feature((165, 185, 206, 213), 0.55)
pink = (hue > 300) | (hue < 25)
mouth_m = feature((86, 257, 210, 308), 0.5, extra=pink & (sat > 0.15) & (val < 0.97))
# Keep the mouth out of the nose: nothing above its top edge.
mouth_m[:258, :] = 0
face_m = cv2.dilate(eyeL_m | eyeR_m | mouth_m, np.ones((5, 5), np.uint8))
head_rgb = cv2.inpaint(rgb, face_m, 7, cv2.INPAINT_TELEA)
head_rgb = cv2.GaussianBlur(head_rgb, (0, 0), 1.2) * (face_m[..., None] > 0) + head_rgb * (face_m[..., None] == 0)
head_rgb = head_rgb.astype(np.uint8)

# ---- the closed mouth: a dark line along the top of the open one ----
smile = np.zeros((H, W), np.float32)
cols = [x for x in range(W) if mouth_m[:, x].any()]
pts = []
for x in cols:
    ys = np.nonzero(mouth_m[:, x])[0]
    pts.append((x, ys.min() + 1.5))
pts = np.array(pts, np.float32)
# Smooth it, and turn the corners up a little.
k = 7
sm = np.convolve(np.pad(pts[:, 1], k, mode="edge"), np.ones(2 * k + 1) / (2 * k + 1), mode="same")[k:-k]
x0, x1 = pts[0, 0], pts[-1, 0]
lift = np.clip(1 - (pts[:, 0] - x0) / 9, 0, 1) ** 2 * 3 + np.clip(1 - (x1 - pts[:, 0]) / 9, 0, 1) ** 2 * 3
line = np.stack([pts[:, 0], sm - lift], axis=1).round().astype(np.int32) * 4
smile_big = np.zeros((H * 4, W * 4), np.uint8)
cv2.polylines(smile_big, [line.reshape(-1, 1, 2)], False, 255, thickness=11, lineType=cv2.LINE_AA)
smile = cv2.resize(smile_big, (W, H), interpolation=cv2.INTER_AREA).astype(np.float32) / 255.0
smile_rgb = np.zeros_like(rgb); smile_rgb[...] = (58, 32, 22)

def soft(m):
    return cv2.GaussianBlur(m.astype(np.float32) / 255.0, (0, 0), 0.7)

layers = {
    "tail": (tail_rgb, tail_a),
    "body": (body_rgb, body_a),
    "pawL": (rgb, paws["pawL"]),
    "pawR": (rgb, paws["pawR"]),
    "head": (head_rgb, head_a),
    "eyeL": (rgb, soft(eyeL_m) * alpha),
    "eyeR": (rgb, soft(eyeR_m) * alpha),
    "mouth": (rgb, soft(mouth_m) * alpha),
    "smile": (smile_rgb, smile),
}

# ---- pack them, scaled down, into one picture ----
SCALE = 0.7
pad = 2
packed, rects, x, rowh, y, sheet_w = [], {}, pad, 0, pad, 512
for name, (c, a) in layers.items():
    ys, xs = np.nonzero(a > 0.01)
    x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
    img = np.dstack([c[y0:y1, x0:x1], (a[y0:y1, x0:x1] * 255).astype(np.uint8)])
    pil = Image.fromarray(img, "RGBA")
    w, h = max(1, round((x1 - x0) * SCALE)), max(1, round((y1 - y0) * SCALE))
    pil = pil.resize((w, h), Image.Resampling.LANCZOS)
    if x + w + pad > sheet_w:
        x, y, rowh = pad, y + rowh + pad, 0
    packed.append((pil, x, y))
    # where it sits on the dog (in the original's pixels), and where it is on the sheet
    rects[name] = {"sheet": [x, y, w, h], "at": [int(x0), int(y0), int(x1 - x0), int(y1 - y0)]}
    x += w + pad
    rowh = max(rowh, h)
sheet = Image.new("RGBA", (sheet_w, y + rowh + pad), (0, 0, 0, 0))
for pil, px, py in packed:
    sheet.alpha_composite(pil, (px, py))
sheet.save(os.path.join(OUT, "dog-parts.png"), optimize=True)
meta = {
    "size": [W, H], "scale": SCALE, "parts": rects,
    # where things turn: the tail at its base, the head on its neck, the
    # paws from the leg, the body from between the paws on the ground
    "pivots": {"tail": [272, 402], "head": [150, 300], "body": [150, 476], "pawL": [91, 432], "pawR": [186, 432]},
    "ground": 478,
}
json.dump(meta, open(os.path.join(OUT, "dog-parts.json"), "w"), indent=1)
print("parts:", sheet.size, {k: v["at"] for k, v in rects.items()})
# The page draws them from the same layout: put it in dog.js.
js_path = os.path.join(ROOT, "src", "phone", "dog.js")
js = open(js_path, encoding="utf-8").read()
start = js.index("  const ART = ")
end = js.index("\n", start)
layout = {"size": meta["size"], "scale": meta["scale"], "ground": meta["ground"], "pivots": meta["pivots"], "parts": meta["parts"]}
js = js[:start] + "  const ART = " + json.dumps(layout, separators=(",", ":")) + ";" + js[end:]
open(js_path, "w", encoding="utf-8").write(js)
