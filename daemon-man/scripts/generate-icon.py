"""生成 DaemonMan 应用图标源图（1024x1024 PNG）。

用法：
    python scripts/generate-icon.py
    pnpm tauri icon .develop/branding/icon.png
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
RADIUS = 220
OUT = Path(__file__).resolve().parents[1] / ".develop" / "branding" / "icon.png"


def vertical_gradient(size: int, top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    image = Image.new("RGBA", (size, size))
    pixels = image.load()
    for y in range(size):
        t = y / (size - 1)
        # ease-in-out 让中间过渡更自然
        t = t * t * (3 - 2 * t)
        color = tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        for x in range(size):
            pixels[x, y] = (*color, 255)
    return image


def rounded_mask(size: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle((0, 0, size - 1, size - 1), radius=radius, fill=255)
    return mask


def radial_highlight(size: int, center: tuple[int, int], radius: float, strength: int) -> Image.Image:
    """返回一张叠加用的径向高光图层。"""
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pixels = layer.load()
    cx, cy = center
    for y in range(size):
        for x in range(size):
            distance = math.hypot(x - cx, y - cy)
            if distance > radius:
                continue
            alpha = int(strength * (1 - distance / radius) ** 2)
            pixels[x, y] = (255, 255, 255, alpha)
    return layer


def main() -> None:
    canvas = vertical_gradient(SIZE, (99, 102, 241), (30, 27, 75))
    canvas.putalpha(rounded_mask(SIZE, RADIUS))

    # 左上角柔光
    glow = radial_highlight(SIZE, (300, 220), 620, 70)
    canvas = Image.alpha_composite(canvas, glow)

    # 内描边
    border = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    border_draw = ImageDraw.Draw(border)
    border_draw.rounded_rectangle(
        (18, 18, SIZE - 19, SIZE - 19),
        radius=RADIUS - 18,
        outline=(199, 210, 254, 90),
        width=10,
    )
    canvas = Image.alpha_composite(canvas, border)

    # DAG 连线
    graph = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    graph_draw = ImageDraw.Draw(graph)
    left = (312, 336)
    right = (712, 336)
    bottom = (512, 700)
    line_color = (199, 210, 254, 235)
    for start, end in ((left, bottom), (right, bottom)):
        graph_draw.line((start, end), fill=line_color, width=26)
        graph_draw.ellipse(
            (start[0] - 13, start[1] - 13, start[0] + 13, start[1] + 13),
            fill=line_color,
        )
        graph_draw.ellipse(
            (end[0] - 13, end[1] - 13, end[0] + 13, end[1] + 13),
            fill=line_color,
        )
    canvas = Image.alpha_composite(canvas, graph)

    # 节点
    nodes = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    node_draw = ImageDraw.Draw(nodes)
    # 底部节点的外圈光晕
    node_draw.ellipse(
        (bottom[0] - 118, bottom[1] - 118, bottom[0] + 118, bottom[1] + 118),
        outline=(165, 180, 252, 110),
        width=12,
    )
    for center in (left, right, bottom):
        radius = 74
        node_draw.ellipse(
            (
                center[0] - radius,
                center[1] - radius,
                center[0] + radius,
                center[1] + radius,
            ),
            fill=(255, 255, 255, 255),
        )
        inner = 26
        node_draw.ellipse(
            (
                center[0] - inner,
                center[1] - inner,
                center[0] + inner,
                center[1] + inner,
            ),
            fill=(79, 70, 229, 255),
        )
    canvas = Image.alpha_composite(canvas, nodes)

    # 轻微阴影增强边缘
    shadow = canvas.filter(ImageFilter.GaussianBlur(0))
    canvas = Image.alpha_composite(shadow, canvas)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(OUT, format="PNG")
    print(f"icon written to {OUT}")


if __name__ == "__main__":
    main()
