#!/usr/bin/env python3
"""PDF text and image extraction for convert2.

Usage:
    convert2_pdf.py extract_text <pdf_path> [--pages 1,2,3]
    convert2_pdf.py extract_images <pdf_path> <output_dir> [--pages 1,2,3]
    convert2_pdf.py render_page <pdf_path> <output_path> --page <n> [--dpi 150]

All commands output JSON to stdout.
"""

import argparse
import json
import os
import sys

import pymupdf


def extract_text(pdf_path: str, pages: list[int] | None = None) -> dict:
    doc = pymupdf.open(pdf_path)
    result = {"total_pages": len(doc), "pages": []}

    for i, page in enumerate(doc):
        if pages is not None and (i + 1) not in pages:
            continue
        text = page.get_text("text")
        result["pages"].append({
            "page_number": i + 1,
            "text": text,
        })

    doc.close()
    return result


def extract_images(pdf_path: str, output_dir: str, pages: list[int] | None = None) -> dict:
    doc = pymupdf.open(pdf_path)
    os.makedirs(output_dir, exist_ok=True)
    images = []

    for i, page in enumerate(doc):
        if pages is not None and (i + 1) not in pages:
            continue
        image_list = page.get_images(full=True)

        for img_index, img_info in enumerate(image_list):
            xref = img_info[0]
            base_image = doc.extract_image(xref)
            if base_image is None:
                continue

            ext = base_image["ext"]
            image_bytes = base_image["image"]
            width = base_image["width"]
            height = base_image["height"]

            filename = f"page{i + 1}_img{img_index + 1}.{ext}"
            filepath = os.path.join(output_dir, filename)

            with open(filepath, "wb") as f:
                f.write(image_bytes)

            images.append({
                "page_number": i + 1,
                "image_index": img_index + 1,
                "filename": filename,
                "filepath": filepath,
                "width": width,
                "height": height,
                "ext": ext,
                "size_bytes": len(image_bytes),
            })

    doc.close()
    return {"total_images": len(images), "images": images}


def render_page(pdf_path: str, page_number: int, output_path: str, dpi: int = 150) -> dict:
    doc = pymupdf.open(pdf_path)
    if page_number < 1 or page_number > len(doc):
        doc.close()
        raise ValueError(f"Page {page_number} out of range (1-{len(doc)})")

    page = doc[page_number - 1]
    zoom = dpi / 72
    mat = pymupdf.Matrix(zoom, zoom)
    pix = page.get_pixmap(matrix=mat)

    os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)
    pix.save(output_path)

    result = {
        "page_number": page_number,
        "filepath": output_path,
        "width": pix.width,
        "height": pix.height,
        "dpi": dpi,
    }

    doc.close()
    return result


def parse_pages(pages_str: str | None) -> list[int] | None:
    if pages_str is None:
        return None
    return [int(p.strip()) for p in pages_str.split(",")]


def main():
    parser = argparse.ArgumentParser(description="convert2 PDF processor")
    subparsers = parser.add_subparsers(dest="command", required=True)

    p_text = subparsers.add_parser("extract_text")
    p_text.add_argument("pdf_path")
    p_text.add_argument("--pages", type=str, default=None)

    p_images = subparsers.add_parser("extract_images")
    p_images.add_argument("pdf_path")
    p_images.add_argument("output_dir")
    p_images.add_argument("--pages", type=str, default=None)

    p_render = subparsers.add_parser("render_page")
    p_render.add_argument("pdf_path")
    p_render.add_argument("output_path")
    p_render.add_argument("--page", type=int, required=True)
    p_render.add_argument("--dpi", type=int, default=150)

    args = parser.parse_args()

    try:
        if args.command == "extract_text":
            result = extract_text(args.pdf_path, parse_pages(args.pages))
        elif args.command == "extract_images":
            result = extract_images(args.pdf_path, args.output_dir, parse_pages(args.pages))
        elif args.command == "render_page":
            result = render_page(args.pdf_path, args.page, args.output_path, args.dpi)
        else:
            parser.print_help()
            sys.exit(1)

        json.dump(result, sys.stdout, indent=2)
    except Exception as e:
        json.dump({"error": str(e)}, sys.stdout, indent=2)


if __name__ == "__main__":
    main()
