#!/usr/bin/env python3
"""Compare two screenshots using pixel-level similarity (MAE).

Usage: screenshot-compare --expected <path> --actual <path> [--threshold 0.95]
Exit code 0 if similarity >= threshold, 1 otherwise.
"""
import argparse
import sys

def compare_images(expected_path, actual_path, threshold):
    try:
        from PIL import Image, ImageChops, ImageStat
    except ImportError:
        print("ERROR: Pillow not installed. Install with: pip3 install Pillow")
        sys.exit(2)

    img1 = Image.open(expected_path).convert('RGB')
    img2 = Image.open(actual_path).convert('RGB')

    if img1.size != img2.size:
        print(f"FAIL: Image sizes differ ({img1.size} vs {img2.size})")
        return False

    if img1.size[0] == 0 or img1.size[1] == 0:
        print("FAIL: Empty images")
        return False

    # Per-channel MAE using PIL's C extension (fast, no Python-level pixel iteration)
    diff = ImageChops.difference(img1, img2)
    stat = ImageStat.Stat(diff)
    mae = sum(stat.mean) / (len(stat.mean) * 255.0)
    similarity = 1.0 - mae

    print(f"Similarity: {similarity:.4f} (threshold: {threshold})")
    return similarity >= threshold


def main():
    parser = argparse.ArgumentParser(description='Compare screenshots')
    parser.add_argument('--expected', required=True, help='Path to expected screenshot')
    parser.add_argument('--actual', required=True, help='Path to actual screenshot')
    parser.add_argument('--threshold', type=float, default=0.95, help='Similarity threshold (0-1)')
    args = parser.parse_args()

    passed = compare_images(args.expected, args.actual, args.threshold)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
