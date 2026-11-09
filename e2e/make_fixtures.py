"""Builds real sample files for the end-to-end run, so no binary fixtures live in the repo."""
import sys
import zipfile
from pathlib import Path

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)

REPORT = [
    "Bracket cracking on press line 4",
    "On 12 March 2026 line 4 at Penang scrapped 3.5% of PN-4471 brackets.",
    "The target is 1.0% scrap.",
    "Die 4 was found worn beyond its limit after testing three candidate causes.",
    "Temporary containment: all parts sorted from 13 March 2026, owner Sam.",
]


def pdf(path, pages):
    objects = ["<< /Type /Catalog /Pages 2 0 R >>", "", "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
    kids = []
    for i, lines in enumerate(pages):
        page_obj = 4 + i * 2
        kids.append(f"{page_obj} 0 R")
        body = "".join(f"BT /F1 12 Tf 50 {720 - n * 18} Td ({l}) Tj ET\n" for n, l in enumerate(lines))
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {page_obj + 1} 0 R /Resources << /Font << /F1 3 0 R >> >> >>")
        objects.append(f"<< /Length {len(body)} >>\nstream\n{body}endstream")
    objects[1] = f"<< /Type /Pages /Kids [{' '.join(kids)}] /Count {len(pages)} >>"
    data = "%PDF-1.4\n"
    offsets = []
    for i, o in enumerate(objects):
        offsets.append(len(data))
        data += f"{i + 1} 0 obj\n{o}\nendobj\n"
    xref = len(data)
    data += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n"
    for off in offsets:
        data += f"{off:010} 00000 n \n"
    data += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    path.write_bytes(data.encode("latin-1"))


def zipped(path, entries):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, body in entries.items():
            z.writestr(name, body)


def docx(path, title, paragraphs):
    p = f'<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>{title}</w:t></w:r></w:p>'
    p += "".join(f"<w:p><w:r><w:t>{t}</w:t></w:r></w:p>" for t in paragraphs)
    zipped(path, {
        "[Content_Types].xml": '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
        "_rels/.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
        "word/document.xml": f'<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{p}</w:body></w:document>',
    })


def xlsx(path):
    rows = [("Week", "Scrap percent"), (10, 3.5), (11, 3.2), (12, 2.8)]
    def cell(ref, v):
        return f'<c r="{ref}" t="inlineStr"><is><t>{v}</t></is></c>' if isinstance(v, str) else f'<c r="{ref}"><v>{v}</v></c>'
    sd = "".join(f'<row r="{i + 1}">{cell(f"A{i + 1}", a)}{cell(f"B{i + 1}", b)}</row>' for i, (a, b) in enumerate(rows))
    zipped(path, {
        "[Content_Types].xml": '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>',
        "_rels/.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
        "xl/workbook.xml": '<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Scrap" sheetId="1" r:id="rId1"/></sheets></workbook>',
        "xl/_rels/workbook.xml.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>',
        "xl/worksheets/sheet1.xml": f'<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sd}</sheetData></worksheet>',
    })


def pptx(path, slides):
    sld = lambda t: f'<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>'
    entries = {
        "ppt/presentation.xml": '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst>' + "".join(f'<p:sldId id="{256 + i}" r:id="rId{i + 1}"/>' for i in range(len(slides))) + "</p:sldIdLst></p:presentation>",
        "ppt/_rels/presentation.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + "".join(f'<Relationship Id="rId{i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i + 1}.xml"/>' for i in range(len(slides))) + "</Relationships>",
    }
    for i, s in enumerate(slides):
        entries[f"ppt/slides/slide{i + 1}.xml"] = sld(s)
    zipped(path, entries)


pdf(out / "8D report.pdf", [REPORT[:3], REPORT[3:]])
docx(out / "Investigation.docx", "Investigation", ["Press line 4 stopped twice during the week of 12 March 2026."])
xlsx(out / "Scrap data.xlsx")
pptx(out / "Review deck.pptx", ["Result after the die change: scrap fell to 0.9% in April 2026."])
(out / "Change note.txt").write_text(
    "Work instruction WI-204 was updated on 2 April 2026.\nThe audit measured scrap at 4.8% on 12 March 2026, not 3.5%.\n",
    encoding="utf-8",
)
long_text = "\n".join(f"Observation {i}: line 4 was inspected on shift {i} and the operators recorded the state of the die and the fixtures in the shift log for later review by the team." for i in range(1, 121))
(out / "Long log.txt").write_text(long_text, encoding="utf-8")
print("fixtures in", out)
