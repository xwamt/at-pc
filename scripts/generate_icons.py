import math
import struct
import zlib
import os

def create_png(width, height, rgba_data):
    def chunk(tag, data):
        return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data) & 0xffffffff)

    header = b'\x89PNG\r\n\x1a\n'
    ihdr = chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
    raw_rows = []
    for y in range(height):
        raw_rows.append(b'\x00' + rgba_data[y * width * 4:(y + 1) * width * 4])
    idat = chunk(b'IDAT', zlib.compress(b''.join(raw_rows), 9))
    iend = chunk(b'IEND', b'')
    return header + ihdr + idat + iend

def dist_segment(px, py, ax, ay, bx, by):
    abx = bx - ax
    aby = by - ay
    apx = px - ax
    apy = py - ay
    ab_len_sq = abx * abx + aby * aby
    if ab_len_sq == 0:
        return math.hypot(apx, apy)
    t = max(0.0, min(1.0, (apx * abx + apy * aby) / ab_len_sq))
    closest_x = ax + t * abx
    closest_y = ay + t * aby
    return math.hypot(px - closest_x, py - closest_y)

def dist_arc(px, py, cx, cy, radius, min_angle, max_angle):
    angle = math.atan2(py - cy, px - cx)
    if min_angle <= angle <= max_angle:
        return abs(math.hypot(px - cx, py - cy) - radius)
    # distance to endpoints
    p1x = cx + radius * math.cos(min_angle)
    p1y = cy + radius * math.sin(min_angle)
    p2x = cx + radius * math.cos(max_angle)
    p2y = cy + radius * math.sin(max_angle)
    return min(math.hypot(px - p1x, py - p1y), math.hypot(px - p2x, py - p2y))

def dist_rounded_rect(px, py, x, y, w, h, r):
    # center
    cx = x + w / 2.0
    cy = y + h / 2.0
    dx = abs(px - cx) - (w / 2.0 - r)
    dy = abs(py - cy) - (h / 2.0 - r)
    ax = max(0.0, dx)
    ay = max(0.0, dy)
    dist_outside = math.hypot(ax, ay)
    dist_inside = min(0.0, max(dx, dy))
    return dist_outside + dist_inside - r

def render_logo(size):
    scale = size / 128.0
    rgba = bytearray(size * size * 4)

    # Super-sampling 2x2 for smooth edges
    samples = [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)]

    # Colors
    c_bg = (23, 32, 51)       # #172033
    c_cyan = (0, 194, 209)     # #00C2D1
    c_white = (255, 255, 255)  # #FFFFFF
    c_green = (53, 208, 127)   # #35D07F

    for py in range(size):
        for px in range(size):
            accum_r, accum_g, accum_b, accum_a = 0.0, 0.0, 0.0, 0.0
            
            for sx, sy in samples:
                x = (px + 0.5 + sx) / scale
                y = (py + 0.5 + sy) / scale

                # 1. Background rounded rect
                d_bg = dist_rounded_rect(x, y, 10, 10, 108, 108, 26)
                if d_bg > 0.5:
                    continue
                
                alpha_bg = max(0.0, min(1.0, 0.5 - d_bg))
                cur_r, cur_g, cur_b = [c * alpha_bg for c in c_bg]
                cur_a = alpha_bg

                # 2. Chevron `>`
                d_chev1 = dist_segment(x, y, 34, 43, 56, 64)
                d_chev2 = dist_segment(x, y, 56, 64, 34, 85)
                d_chev = min(d_chev1, d_chev2) - 5.5
                if d_chev < 0.5:
                    a = max(0.0, min(1.0, 0.5 - d_chev))
                    # blend cyan over bg
                    cur_r = cur_r * (1.0 - a) + c_cyan[0] * a
                    cur_g = cur_g * (1.0 - a) + c_cyan[1] * a
                    cur_b = cur_b * (1.0 - a) + c_cyan[2] * a

                # 3. Letter `P`
                # Stem: (73, 43) to (73, 80)
                d_stem = dist_segment(x, y, 73, 43, 73, 80) - 5.25
                # Top bar: (73, 43) to (85, 43)
                d_top = dist_segment(x, y, 73, 43, 85, 43) - 5.25
                # Arc: center (85, 56), radius 13, angle -pi/2 to pi/2
                d_arc = dist_arc(x, y, 85, 56, 13, -math.pi/2, math.pi/2) - 5.25
                # Bottom bar of loop: (73, 69) to (85, 69)
                d_bot = dist_segment(x, y, 73, 69, 85, 69) - 5.25
                d_p = min(d_stem, d_top, d_arc, d_bot)
                if d_p < 0.5:
                    a = max(0.0, min(1.0, 0.5 - d_p))
                    # blend white
                    cur_r = cur_r * (1.0 - a) + c_white[0] * a
                    cur_g = cur_g * (1.0 - a) + c_white[1] * a
                    cur_b = cur_b * (1.0 - a) + c_white[2] * a

                # 4. Accent `_`
                d_under = dist_segment(x, y, 72, 88, 99, 88) - 5.25
                if d_under < 0.5:
                    a = max(0.0, min(1.0, 0.5 - d_under))
                    # blend green
                    cur_r = cur_r * (1.0 - a) + c_green[0] * a
                    cur_g = cur_g * (1.0 - a) + c_green[1] * a
                    cur_b = cur_b * (1.0 - a) + c_green[2] * a

                accum_r += cur_r
                accum_g += cur_g
                accum_b += cur_b
                accum_a += cur_a

            idx = (py * size + px) * 4
            rgba[idx + 0] = int(round(accum_r / 4.0))
            rgba[idx + 1] = int(round(accum_g / 4.0))
            rgba[idx + 2] = int(round(accum_b / 4.0))
            rgba[idx + 3] = int(round(accum_a / 4.0 * 255.0))

    return create_png(size, size, bytes(rgba))

def create_ico(png_map):
    # png_map is dict of {size: png_bytes}
    sizes = sorted(png_map.keys())
    num_images = len(sizes)
    header = struct.pack('<HHH', 0, 1, num_images)
    offset = 6 + 16 * num_images

    entries = []
    data_blobs = []
    for s in sizes:
        png_data = png_map[s]
        w = 0 if s == 256 else s
        h = 0 if s == 256 else s
        entry = struct.pack('<BBBBHHII', w, h, 0, 0, 1, 32, len(png_data), offset)
        entries.append(entry)
        data_blobs.append(png_data)
        offset += len(png_data)

    return header + b''.join(entries) + b''.join(data_blobs)

def main():
    os.makedirs('media', exist_ok=True)
    
    png_images = {}
    for s in [256, 128, 64, 48, 32, 16]:
        print(f"Rendering icon {s}x{s}...")
        png_data = render_logo(s)
        png_images[s] = png_data

    # Save 128x128 standard
    with open('media/at-pc-icon.png', 'wb') as f:
        f.write(png_images[128])

    # Save 256x256 high res
    with open('media/at-pc-icon-256.png', 'wb') as f:
        f.write(png_images[256])

    # Save Windows .ico
    ico_data = create_ico(png_images)
    with open('media/at-pc.ico', 'wb') as f:
        f.write(ico_data)

    print("Successfully generated media/at-pc-icon.png, media/at-pc-icon-256.png, and media/at-pc.ico!")

if __name__ == '__main__':
    main()
