"""Draws PDF pages to PNGs with the app's own reader, so exports can be looked at. Usage: pdf_pages.py <exe> <pdf> <outdir> [pages]"""
import base64
import json
import re
import subprocess
import sys
from pathlib import Path

exe, pdf, outdir = str(Path(sys.argv[1]).resolve()), sys.argv[2], sys.argv[3]
Path(outdir).mkdir(parents=True, exist_ok=True)
raw = Path(pdf).read_bytes()
count = len(re.findall(rb"/Type\s*/Page[^s]", raw))
print(f"{Path(pdf).name}: {count} pages, outline: {b'/Outlines' in raw}")
for page in range(1, min(count, int(sys.argv[4]) if len(sys.argv) > 4 else 3) + 1):
    out = subprocess.run([exe, "--a3-images", pdf, Path(pdf).name, json.dumps({"page": page})], capture_output=True, check=True).stdout
    png = base64.b64decode(json.loads(out)[0]["base64"])
    target = Path(outdir) / f"{Path(pdf).stem}-p{page}.png"
    target.write_bytes(png)
    print(" ", target)
