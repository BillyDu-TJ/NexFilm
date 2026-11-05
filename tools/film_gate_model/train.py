"""Train the film-gate corner verifier (and a pure-CNN edge regressor for comparison)
with leave-one-roll-out folds.

Data (labels, previews, runs) live outside the repo; pass --work to point at them.
  python train.py --work /path/to/work2 verifier
  python train.py --work /path/to/work2 regressor
  python train.py --work /path/to/work2 box_scorer --pool box_pool.jsonl
"""
import argparse
import json
import os
import sys

import numpy as np
import torch
import torch.nn as nn
from PIL import Image

PATCH = 48
# Patch normalisation. "base" (shipped, as in film_border.rs): gains that turn
# the mean colour outside the quad grey, then one mean/spread over all three
# channels. For comparison: "global" (that last step alone, the earlier input)
# and "channel" (each channel by its own mean/spread).
NORM = "base"
torch.backends.cudnn.deterministic = True


def load_frames(work):
    labels = json.load(open(os.path.join(work, "labels/FROZEN.json")))
    data_root = os.path.dirname(work.rstrip("/"))
    manifest = json.load(open(os.path.join(data_root, "manifest.json")))
    paths = {}
    for m in manifest:
        stem = m["preview"].split("/")[-1][:-4]
        paths[m["roll"] + "/" + stem] = os.path.join(data_root, m["preview"])
    frames = []
    for k, v in sorted(labels.items()):
        if v["provenance"] == "excluded":
            continue
        rgb = np.asarray(Image.open(paths[k]).convert("RGB"), dtype=np.float32)
        frames.append({"id": k, "roll": v["roll"], "quad": np.array(v["quad"]), "rgb": rgb})
    return frames, paths


# ---- verifier -------------------------------------------------------------

class Verifier(nn.Module):
    def __init__(self):
        super().__init__()
        self.c1 = nn.Conv2d(4, 8, 3, padding=1)
        self.c2 = nn.Conv2d(8, 16, 3, padding=1)
        self.c3 = nn.Conv2d(16, 16, 3, padding=1)
        self.fc = nn.Linear(16 * 6 * 6, 1)

    def forward(self, x):
        x = torch.max_pool2d(torch.relu(self.c1(x)), 2)
        x = torch.max_pool2d(torch.relu(self.c2(x)), 2)
        x = torch.max_pool2d(torch.relu(self.c3(x)), 2)
        return self.fc(x.flatten(1)).squeeze(1)


def corner_patch(rgb, quad, k):
    """48x48x4 patch centred on corner k of quad (TL TR BR BL), oriented so the
    quad interior lies to the bottom right. Channels: RGB white-balanced on the
    base and normalised (see NORM), and an interior mask. Mirrors film_border.rs
    `verifier_patch`."""
    h, w, _ = rgb.shape
    cx, cy = quad[k]
    half = PATCH // 2
    x0 = int(np.floor(cx)) - half
    y0 = int(np.floor(cy)) - half
    xs = np.clip(np.arange(x0, x0 + PATCH), 0, w - 1)
    ys = np.clip(np.arange(y0, y0 + PATCH), 0, h - 1)
    patch = rgb[ys][:, xs].copy()
    # interior mask: point inside the convex quad
    gx, gy = np.meshgrid(np.arange(x0, x0 + PATCH) + 0.5, np.arange(y0, y0 + PATCH) + 0.5)
    inside = np.ones_like(gx, dtype=bool)
    for a in range(4):
        p, q = quad[a], quad[(a + 1) % 4]
        cross = (q[0] - p[0]) * (gy - p[1]) - (q[1] - p[1]) * (gx - p[0])
        inside &= cross >= 0
    if NORM == "base" and (~inside).any():
        # white-balance on the base: the mean colour outside the quad becomes grey
        base = np.maximum(patch[~inside].mean(0), 1.0)
        patch = patch * (base.mean() / base)
    axes = (0, 1) if NORM == "channel" else None
    mean = patch.mean(axes)
    std = patch.std(axes) + 8.0
    patch = (patch - mean) / std
    out = np.concatenate([patch, inside[..., None].astype(np.float32)], axis=2)
    if k in (1, 2):
        out = out[:, ::-1]
    if k in (2, 3):
        out = out[::-1]
    return np.ascontiguousarray(out.transpose(2, 0, 1))


def candidates(frame, rng, runs):
    q = frame["quad"]
    out = []
    for _ in range(6):
        out.append(q + rng.uniform(-2, 2, q.shape))
    for _ in range(14):
        out.append(q + rng.uniform(-30, 30, q.shape) * (rng.random((4, 1)) < 0.6))
    for _ in range(10):
        side = rng.integers(4)
        a, b = side, (side + 1) % 4
        centre = q.mean(0)
        frac = rng.uniform(0.04, 0.45) * (1 if rng.random() < 0.8 else -0.3)
        qq = q.copy()
        qq[a] = q[a] + (centre - (q[a] + q[b]) / 2) * frac * 2
        qq[b] = q[b] + (centre - (q[a] + q[b]) / 2) * frac * 2
        out.append(qq)
    for run in runs:
        if frame["id"] in run:
            out.append(run[frame["id"]])
    out.extend(POOL.get(frame["id"], []))
    return out


# Ranked boxes of the search (`film_gate_eval --candidates`), by frame id: the
# wrong boxes the verifier must turn down when it picks among them.
POOL = {}


def load_pool(path):
    for line in open(path):
        r = json.loads(line)
        POOL[r["id"]] = [np.array(q) * [640, 424] for q in r.get("ranked") or []]


def quad_mask(quad, h, w):
    gx, gy = np.meshgrid(np.arange(w) + 0.5, np.arange(h) + 0.5)
    inside = np.ones((h, w), dtype=bool)
    for a in range(4):
        p, q = quad[a], quad[(a + 1) % 4]
        inside &= (q[0] - p[0]) * (gy - p[1]) - (q[1] - p[1]) * (gx - p[0]) >= 0
    return inside


def thin_slab(frame, rng):
    """A copy of the frame whose picture, in a slab along one side, is pulled
    towards the base colour: a thin (nearly unexposed) area of the negative.
    The true corners there become faint, and the slab's
    inner edge is a picture-internal line whose box shares three true sides.
    Returns the image and its candidates (true corners and that wrong box)."""
    q = frame["quad"]
    rgb = frame["rgb"]
    h, w, _ = rgb.shape
    centre = q.mean(0)
    inside = quad_mask(q, h, w)
    ring = quad_mask(centre + (q - centre) * 1.06, h, w) & ~inside
    base = rgb[ring].mean(0)
    side = rng.integers(4)
    a, b = side, (side + 1) % 4
    frac = rng.uniform(0.08, 0.45)
    moved = q.copy()
    moved[a] = q[a] + (centre - (q[a] + q[b]) / 2) * frac * 2
    moved[b] = q[b] + (centre - (q[a] + q[b]) / 2) * frac * 2
    slab = inside & ~quad_mask(moved, h, w)
    alpha = rng.uniform(0.6, 0.95)
    out = rgb.copy()
    out[slab] = base + (out[slab] - base) * (1 - alpha)
    cands = [q + rng.uniform(-2, 2, q.shape) for _ in range(2)]
    cands += [moved] + [moved + rng.uniform(-2, 2, q.shape) for _ in range(2)]
    return out, cands


def build_set(frames, rng, runs, slabs=0):
    xs, ys = [], []
    for f in frames:
        images = [(f["rgb"], candidates(f, rng, runs))]
        images += [thin_slab(f, rng) for _ in range(slabs)]
        for rgb, cands in images:
            for cq in cands:
                err = np.hypot(*(cq - f["quad"]).T)
                for k in range(4):
                    if 3.0 < err[k] < 7.0:
                        continue
                    xs.append(corner_patch(rgb, cq, k))
                    ys.append(1.0 if err[k] <= 3.0 else 0.0)
    return np.stack(xs), np.array(ys, np.float32)


def augment(x, rng):
    x = x.copy()
    t = rng.random(len(x)) < 0.5
    x[t] = x[t].transpose(0, 1, 3, 2)
    gain = rng.uniform(0.8, 1.25, (len(x), 3, 1, 1)).astype(np.float32)
    x[:, :3] = x[:, :3] * gain + rng.normal(0, 0.05, (len(x), 3, 1, 1)).astype(np.float32)
    return x


def train_verifier(x, y, seed, device, epochs=12):
    torch.manual_seed(seed)
    rng = np.random.default_rng(seed)
    model = Verifier().to(device)
    opt = torch.optim.Adam(model.parameters(), lr=2e-3, weight_decay=1e-4)
    pos = y.mean()
    lossf = nn.BCEWithLogitsLoss(pos_weight=torch.tensor((1 - pos) / max(pos, 1e-3), device=device))
    for ep in range(epochs):
        order = rng.permutation(len(x))
        for i in range(0, len(x), 256):
            idx = order[i:i + 256]
            xb = torch.from_numpy(augment(x[idx], rng)).to(device)
            yb = torch.from_numpy(y[idx]).to(device)
            opt.zero_grad()
            loss = lossf(model(xb), yb)
            loss.backward()
            opt.step()
    return model.eval()


def verify_quad(model, rgb, quad, device):
    patches = np.stack([corner_patch(rgb, quad, k) for k in range(4)])
    with torch.no_grad():
        p = torch.sigmoid(model(torch.from_numpy(patches).to(device))).cpu().numpy()
    return float(p.min()), p.tolist()


def load_run(path):
    d = {}
    for line in open(path):
        r = json.loads(line)
        d[r["id"]] = np.array(r["points"]) * [640, 424]
    return d


# ---- box scorer -------------------------------------------------------------
# Chooses the Low-confidence box among the search's first 12 boxes; it trains on
# the first 24 for more wrong examples (the order within 12 is the same). Mirrors
# film_border.rs `box_scorer_preference`: the image as 16 px cell means, the
# box's coverage of 32 px cells joined after the first layer, and the box's
# |ln aspect|, area share and step score. One file holds an ensemble of seeds.

SCORED_BOXES = 24
SCORER_SEEDS = 3


class BoxScorer(nn.Module):
    def __init__(self, width=8, hidden=16):
        super().__init__()
        self.c1 = nn.Conv2d(3, width, 3, padding=1)
        self.c2 = nn.Conv2d(width + 1, 2 * width, 3, padding=1)
        self.c3 = nn.Conv2d(2 * width, 2 * width, 3, padding=1)
        self.c4 = nn.Conv2d(2 * width, 2 * width, 3, padding=1)
        self.f = nn.Linear(4 * width + 3, hidden)
        self.o = nn.Linear(hidden, 1)

    def forward(self, image, mask, geometry):
        x = torch.max_pool2d(torch.relu(self.c1(image)), 2)
        x = torch.max_pool2d(torch.relu(self.c2(torch.cat([x, mask], 1))), 2)
        x = torch.relu(self.c4(torch.relu(self.c3(x))))
        z = torch.cat([x.amax((2, 3)), x.mean((2, 3)), geometry], 1)
        return self.o(torch.relu(self.f(z))).squeeze(1)


def scorer_image(rgb):
    h, w = rgb.shape[0] // 16, rgb.shape[1] // 16
    return rgb[:h * 16, :w * 16].reshape(h, 16, w, 16, 3).mean((1, 3)).transpose(2, 0, 1)


def box_coverage(box, rows, cols, cell=32.0):
    x, y, w, h = [v * 4.0 for v in box]
    over = lambda n, lo, hi: np.clip(np.minimum(np.arange(1, n + 1), hi / cell) - np.maximum(np.arange(n), lo / cell), 0, 1)
    return np.outer(over(rows, y, y + h), over(cols, x, x + w)).astype(np.float32)


def polygon_iou(a, b):
    cross = lambda u, v: u[0] * v[1] - u[1] * v[0]

    def clip(subject, clipper):
        out = list(subject)
        for i in range(4):
            p, q = clipper[i], clipper[(i + 1) % 4]
            inside = lambda r: (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]) >= 0
            points, out = out, []
            for j in range(len(points)):
                c, d = points[j], points[(j + 1) % len(points)]
                if inside(d) != inside(c):
                    e = np.subtract(d, c)
                    t = cross(np.subtract(q, p), np.subtract(p, c)) / cross(np.subtract(q, p), e)
                    out.append(np.add(c, t * e))
                if inside(d):
                    out.append(d)
            if not out:
                return 0.0
        return 0.5 * abs(sum(out[i][0] * out[i - 1][1] - out[i - 1][0] * out[i][1] for i in range(len(out))))
    area = lambda q: 0.5 * abs(sum(q[i][0] * q[i - 1][1] - q[i - 1][0] * q[i][1] for i in range(4)))
    inter = clip(a, b)
    return inter / (area(a) + area(b) - inter)


def usable(quad, label, short):
    return polygon_iou(quad, label) >= 0.95 and np.hypot(*(quad - label).T).max() <= 0.015 * short


def scorer_sets(frames, pool):
    sets = {}
    for f in frames:
        boxes = pool.get(f["id"], [])[:SCORED_BOXES]
        if not boxes:
            continue
        h, w = f["rgb"].shape[:2]
        image = scorer_image(f["rgb"])
        rows, cols = image.shape[1] // 2, image.shape[2] // 2
        pad = SCORED_BOXES - len(boxes)
        sane = [b["quad"] is not None for b in boxes]
        sets[f["id"]] = {
            "image": image,
            "mask": np.stack([box_coverage(b["box"], rows, cols) for b in boxes] + [np.zeros((rows, cols), np.float32)] * pad),
            "geometry": np.array([[abs(np.log(b["box"][2] / b["box"][3])), b["box"][2] * b["box"][3] / ((w // 4) * (h // 4)),
                                   b["score"] / 36.0] for b in boxes] + [[0, 0, 0]] * pad, np.float32),
            "positive": np.array([s and usable(np.array(b["quad"]) * [w, h], f["quad"], min(w, h)) for b, s in zip(boxes, sane)] + [False] * pad),
            "valid": np.array(sane + [False] * pad),
        }
    return sets


def train_box_scorer(sets, seed, device, epochs=40):
    torch.manual_seed(seed)
    rng = np.random.default_rng(seed)
    sets = [s for s in sets if s["positive"].any()]
    model = BoxScorer().to(device)
    opt = torch.optim.Adam(model.parameters(), lr=3e-3, weight_decay=1e-4)
    k = SCORED_BOXES
    for ep in range(epochs):
        order = rng.permutation(len(sets))
        for i in range(0, len(sets), 16):
            batch = [sets[j] for j in order[i:i + 16]]
            gain = rng.uniform(0.8, 1.25, (len(batch), 3, 1, 1))
            images = [s["image"] * g for s, g in zip(batch, gain)]
            images = np.stack([(im - im.mean()) / (im.std() + 8) for im in images]).astype(np.float32)
            t = lambda a: torch.from_numpy(np.ascontiguousarray(a)).to(device)
            x = t(np.repeat(images[:, None], k, 1))
            m = t(np.stack([s["mask"] for s in batch])[:, :, None])
            if rng.random() < 0.5:
                x, m = x.flip(-1), m.flip(-1)
            if rng.random() < 0.5:
                x, m = x.flip(-2), m.flip(-2)
            b = len(batch)
            valid = t(np.stack([s["valid"] for s in batch]))
            positive = t(np.stack([s["positive"] for s in batch])) & valid
            scores = model(x.reshape(b * k, *x.shape[2:]), m.reshape(b * k, *m.shape[2:]),
                           t(np.stack([s["geometry"] for s in batch])).reshape(b * k, 3)).reshape(b, k)
            log_p = torch.log_softmax(scores.masked_fill(~valid, -1e4), 1)
            loss = -torch.logsumexp(log_p.masked_fill(~positive, -1e4), 1).mean()
            opt.zero_grad()
            loss.backward()
            opt.step()
    return model.eval()


# ---- pure CNN regressor -----------------------------------------------------

class Regressor(nn.Module):
    def __init__(self):
        super().__init__()
        ch = [3, 16, 32, 48, 64, 64]
        layers = []
        for a, b in zip(ch[:-1], ch[1:]):
            layers += [nn.Conv2d(a, b, 3, padding=1), nn.BatchNorm2d(b), nn.ReLU(), nn.MaxPool2d(2)]
        self.body = nn.Sequential(*layers)
        self.head = nn.Linear(64 * 4 * 6, 8)

    def forward(self, x):
        return self.head(self.body(x).flatten(1))


def reg_input(rgb):
    img = Image.fromarray(rgb.astype(np.uint8)).resize((192, 128), Image.BILINEAR)
    a = np.asarray(img, np.float32) / 255.0
    a = (a - a.mean()) / (a.std() + 1e-3)
    return a.transpose(2, 0, 1)


def train_regressor(frames, seed, device, epochs=150):
    torch.manual_seed(seed)
    rng = np.random.default_rng(seed)
    x = np.stack([reg_input(f["rgb"]) for f in frames])
    y = np.stack([(f["quad"] / [640, 424]).reshape(-1) for f in frames]).astype(np.float32)
    model = Regressor().to(device)
    opt = torch.optim.Adam(model.parameters(), lr=2e-3, weight_decay=1e-4)
    for ep in range(epochs):
        order = rng.permutation(len(x))
        model.train()
        for i in range(0, len(x), 32):
            idx = order[i:i + 32]
            xb = x[idx].copy()
            yb = y[idx].copy().reshape(-1, 4, 2)
            fh = rng.random(len(idx)) < 0.5
            fv = rng.random(len(idx)) < 0.5
            xb[fh] = xb[fh][:, :, :, ::-1]
            yb[fh, :, 0] = 1 - yb[fh, :, 0]
            yb[fh] = yb[fh][:, [1, 0, 3, 2]]
            xb[fv] = xb[fv][:, :, ::-1, :]
            yb[fv, :, 1] = 1 - yb[fv, :, 1]
            yb[fv] = yb[fv][:, [3, 2, 1, 0]]
            xb = xb * rng.uniform(0.8, 1.2, (len(idx), 3, 1, 1)).astype(np.float32)
            opt.zero_grad()
            loss = nn.functional.smooth_l1_loss(model(torch.from_numpy(np.ascontiguousarray(xb)).to(device)),
                                                torch.from_numpy(yb.reshape(-1, 8)).to(device), beta=0.01)
            loss.backward()
            opt.step()
    return model.eval()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", required=True)
    ap.add_argument("what", choices=["verifier", "regressor", "box_scorer"])
    ap.add_argument("--runs", nargs="*", default=[], help="detector runs used as extra training candidates")
    ap.add_argument("--pool", help="film_gate_eval --candidates output (verifier) or box_pool output (box_scorer)")
    ap.add_argument("--score", nargs="*", default=[], help="runs to score with the held-out fold model")
    ap.add_argument("--norm", choices=["global", "channel", "base"], default=NORM)
    ap.add_argument("--seed", type=int, default=0, help="added to each fold's seed")
    ap.add_argument("--folds", nargs="*", help="train only these held-out rolls")
    ap.add_argument("--slabs", type=int, default=2, help="thin-slab images per training frame")
    ap.add_argument("--epochs", type=int, default=12)
    ap.add_argument("--out", help="model directory (default <work>/models)")
    args = ap.parse_args()
    globals()["NORM"] = args.norm
    device = "cuda" if torch.cuda.is_available() else "cpu"
    frames, _ = load_frames(args.work)
    rolls = sorted(set(f["roll"] for f in frames))
    out_dir = args.out or os.path.join(args.work, "models")
    os.makedirs(out_dir, exist_ok=True)
    runs = [load_run(p) for p in args.runs]
    if args.pool and args.what == "verifier":
        load_pool(args.pool)
    if args.what == "box_scorer":
        pool = {r["id"]: r["boxes"] for r in map(json.loads, open(args.pool))}
        sets = scorer_sets(frames, pool)
    score_runs = {os.path.basename(p): load_run(p) for p in args.score}
    results = {}
    for fold, held in enumerate(rolls + ["ALL"]):
        if args.folds and held not in args.folds:
            continue
        train = [f for f in frames if f["roll"] != held]
        test = [f for f in frames if f["roll"] == held]
        if args.what == "box_scorer":
            chosen = [sets[f["id"]] for f in train if f["id"] in sets]
            state = {}
            for seed in range(SCORER_SEEDS):
                for key, value in train_box_scorer(chosen, seed, device).state_dict().items():
                    state["m%d.%s" % (seed, key)] = value.cpu()
            torch.save(state, os.path.join(out_dir, "box_scorer_%s.pt" % held.replace(" ", "_")))
            print("fold %-28s train %d frames" % (held, len(chosen)), flush=True)
            continue
        if args.what == "verifier":
            x, y = build_set(train, np.random.default_rng(fold + args.seed), runs, args.slabs)
            model = train_verifier(x, y, fold + args.seed, device, args.epochs)
            for name, run in score_runs.items():
                for f in test:
                    s, per = verify_quad(model, f["rgb"], run[f["id"]], device)
                    results.setdefault(name, {})[f["id"]] = {"v": s, "corners": per}
            print("fold %-28s train %6d patches (pos %.2f)" % (held, len(x), y.mean()), flush=True)
        else:
            model = train_regressor(train, fold, device)
            with torch.no_grad():
                for f in test:
                    p = model(torch.from_numpy(reg_input(f["rgb"])[None]).to(device)).cpu().numpy()[0]
                    results.setdefault("regressor", {})[f["id"]] = (p.reshape(4, 2)).tolist()
            print("fold %-28s train %d frames" % (held, len(train)), flush=True)
        torch.save(model.state_dict(), os.path.join(out_dir, "%s_%s.pt" % (args.what, held.replace(" ", "_"))))
    json.dump(results, open(os.path.join(out_dir, "%s_loro.json" % args.what), "w"), ensure_ascii=False)


if __name__ == "__main__":
    main()
