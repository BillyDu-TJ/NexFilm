"""Train the 1-D side network of the 135 strip search in src/film_border.rs.

  python strip.py --work <dir> dump.jsonl --seed 0

`dump.jsonl` comes from `film_gate_eval`'s `strip_dump` binary over the labelled
135 frames, so the networks train on exactly the profiles the detector computes.
Writes strip_<fold>.pt to <work>/models (one per held-out roll, plus ALL), which
export.py picks up next to verifier_<fold>.pt.
"""
import argparse
import json
import math
import os
import random

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

CHANNELS = 8
WIDTH = 16
KERNEL = 5
DILATIONS = [1, 2, 4, 8, 16, 32, 64, 1]
# Shared with the detector: position channel and hole-row distance channel.
POSITION, GAP = 6, 7
# Largest relative scale change of a profile in training (camera distance).
ZOOM = 0.12


class SideNet(nn.Module):
    """Per-position logits of two sides of the picture from a strip profile."""

    def __init__(self):
        super().__init__()
        sizes = [CHANNELS] + [WIDTH] * (len(DILATIONS) - 1) + [2]
        self.layers = nn.ModuleList(
            nn.Conv1d(a, b, KERNEL, dilation=d, padding=d * (KERNEL // 2))
            for a, b, d in zip(sizes, sizes[1:], DILATIONS)
        )

    def forward(self, x):
        for i, layer in enumerate(self.layers):
            x = layer(x)
            if i + 1 < len(self.layers):
                x = F.relu(x)
        return x


class StripNets(nn.Module):
    def __init__(self):
        super().__init__()
        self.along = SideNet()


def strip_sides(quad, transposed):
    """(start, end, top, bottom) of a labelled quad in strip coordinates."""
    along = sorted(p[1] if transposed else p[0] for p in quad)
    across = sorted(p[0] if transposed else p[1] for p in quad)
    return (along[0] + along[1]) / 2, (along[2] + along[3]) / 2, (across[0] + across[1]) / 2, (across[2] + across[3]) / 2


def load(dump, labels):
    frames = []
    for line in open(dump):
        r = json.loads(line)
        lab = labels.get(r["id"])
        if not r.get("strip") or not lab or not lab.get("quad") or lab.get("provenance") == "excluded":
            continue
        s, e, t, b = strip_sides(lab["quad"], r["transposed"])
        frames.append(dict(
            id=r["id"], roll=lab["roll"], gap=r["far"] - r["near"], mid=(r["far"] + r["near"]) / 2,
            along=np.array(r["along_profile"], np.float32).reshape(CHANNELS, -1),
            sides=(s, e, t, b), rust_logits=np.array(r["along_logits"], np.float32)))
    return frames


def augment(profile, first, second, centre, gap, rng, shift_range):
    """Shift, flip, contrast and colour changes of one profile with its two sides."""
    p = profile.copy()
    n = p.shape[1]
    zoom = math.exp(rng.uniform(-ZOOM, ZOOM)) if ZOOM else 1.0
    if zoom != 1.0:
        middle = n / 2
        source = np.clip((np.arange(n) + 0.5 - middle) / zoom + middle - 0.5, 0, n - 1)
        low = np.floor(source).astype(int)
        high = np.minimum(low + 1, n - 1)
        frac = (source - low).astype(np.float32)
        p = p[:, low] * (1 - frac) + p[:, high] * frac
        first, second = (first - middle) * zoom + middle, (second - middle) * zoom + middle
        centre = (centre - middle) * zoom + middle
        gap = gap * zoom
        p[GAP] = gap / n
    shift = rng.randint(-shift_range, shift_range)
    if shift:
        idx = np.clip(np.arange(n) - shift, 0, n - 1)
        p = p[:, idx]
        first, second = first + shift, second + shift
        centre = centre + shift  # the hole rows / strip move with the content
    if rng.random() < 0.5:
        p = p[:, ::-1].copy()
        first, second = n - second, n - first
        centre = n - centre
    p[POSITION] = (np.arange(n) - centre) / gap
    k = rng.uniform(0.7, 1.4)
    p[:6] *= k
    p[:3] += np.array([rng.uniform(-0.15, 0.15) for _ in range(3)], np.float32)[:, None]
    p[3:5] += rng.uniform(-0.1, 0.1)
    return p, first, second


def target(n, pos):
    t = np.zeros(n, np.float32)
    i = int(math.floor(pos))
    f = pos - i
    if 0 <= i < n:
        t[i] += 1 - f
    if 0 <= i + 1 < n:
        t[i + 1] += f
    return t / max(t.sum(), 1e-6)


def batch(frames, rng, size):
    xs, ts = [], []
    for _ in range(size):
        fr = rng.choice(frames)
        s, e, _, _ = fr["sides"]
        n = fr["along"].shape[1]
        p, a, c = augment(fr["along"], s, e, n / 2, fr["gap"], rng, 48)
        xs.append(p)
        ts.append(np.stack([target(n, a), target(n, c)]))
    # Profiles of one orientation share the image's length (640 or 424).
    n = min(x.shape[1] for x in xs)
    return torch.tensor(np.stack([x[:, :n] for x in xs])), torch.tensor(np.stack([t[:, :n] for t in ts]))


def train(frames, seed, steps, device):
    rng = random.Random(seed)
    torch.manual_seed(seed)
    nets = StripNets().to(device)
    opt = torch.optim.Adam(nets.parameters(), lr=2e-3)
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, max_lr=3e-3, total_steps=steps)
    for step in range(steps):
        x, t = batch(frames, rng, 32)
        loss = -(t.to(device) * F.log_softmax(nets.along(x.to(device)), -1)).sum(-1).mean()
        opt.zero_grad()
        loss.backward()
        opt.step()
        sched.step()
        if step % 500 == 0 or step == steps - 1:
            print(f"  step {step} loss {loss.item():.3f}", flush=True)
    return nets.cpu()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", required=True)
    ap.add_argument("dump")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--steps", type=int, default=3000)
    ap.add_argument("--out", default=None, help="models directory (default <work>/models)")
    ap.add_argument("--folds", default=None, help="comma-separated held-out rolls, or ALL")
    args = ap.parse_args()
    labels = json.load(open(os.path.join(args.work, "labels", "FROZEN.json")))
    frames = load(args.dump, labels)
    out = args.out or os.path.join(args.work, "models")
    os.makedirs(out, exist_ok=True)
    rolls = sorted({f["roll"] for f in frames})
    folds = args.folds.split(",") if args.folds else rolls + ["ALL"]
    device = "cuda" if torch.cuda.is_available() else "cpu"
    for fold in folds:
        train_frames = [f for f in frames if f["roll"] != fold]
        print(f"fold {fold}: {len(train_frames)} frames", flush=True)
        nets = train(train_frames, args.seed, args.steps, device)
        torch.save(nets.state_dict(), os.path.join(out, "strip_%s.pt" % fold.replace(" ", "_")))


if __name__ == "__main__":
    main()
