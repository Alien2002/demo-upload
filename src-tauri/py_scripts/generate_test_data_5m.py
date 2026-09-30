"""
Synthetic AP/payments test data for audit-analytics testing (DuckDB / ACL-style commands).

Usage:
  pip install numpy xlsxwriter pyarrow
  python generate_test_data.py                          # 5M rows -> ONE Excel file, 5 sheets x 1M
  python generate_test_data.py --split-files            # 5M rows -> 5 separate Excel files x 1M
  python generate_test_data.py --format parquet         # 5M rows -> one Parquet file (fastest)
  python generate_test_data.py --format csv             # 5M rows -> one CSV file
  python generate_test_data.py --format all             # all three
  python generate_test_data.py --rows 1000000           # any row count

Excel allows max 1,048,576 rows per sheet, so xlsx output is split into chunks of
--sheet-rows (default 1,000,000). By default all sheets go in ONE workbook.
xlsxwriter keeps every sheet's uncompressed XML (~1.6 GB per 1M rows) in temp
files until the workbook closes, so one file needs ~1.7 GB free per 1M rows
(~8.5 GB for 5M). --split-files writes one workbook per chunk and needs only
~1.7 GB at a time.

Temp files go to --tmpdir (default: the output folder, not /tmp, which often has
a small quota). Anomaly counts scale with --rows. Fixed seed => identical data every run.
"""
import argparse, datetime as dt, os, shutil, time
import numpy as np

p = argparse.ArgumentParser()
p.add_argument("--rows", type=int, default=5_000_000)
p.add_argument("--format", choices=["xlsx", "csv", "parquet", "all"], default="xlsx")
p.add_argument("--out", default="test_transactions")
p.add_argument("--sheet-rows", type=int, default=1_000_000)
p.add_argument("--split-files", action="store_true", help="xlsx: one file per chunk instead of one workbook with many sheets")
p.add_argument("--tmpdir", default=None, help="where xlsxwriter puts temp files (default: output folder)")
p.add_argument("--seed", type=int, default=42)
a = p.parse_args()
if a.sheet_rows > 1_048_575:
    p.error("--sheet-rows must be <= 1,048,575 (Excel limit minus header)")
out_dir = os.path.dirname(os.path.abspath(a.out))
tmpdir = os.path.abspath(a.tmpdir or out_dir)
os.makedirs(tmpdir, exist_ok=True)

N = a.rows
rng = np.random.default_rng(a.seed)
t0 = time.time()
def log(msg): print(f"[{time.time()-t0:6.0f}s] {msg}", flush=True)
def pct(x): return max(1, int(N * x))

# ---------------- reference data ----------------
prefixes = ["Kilimanjaro","Serengeti","Zanzibar","Mwanza","Arusha","Dodoma","Tanga","Mbeya","Morogoro","Iringa",
            "Coastal","Lake","Savanna","Baobab","Pwani","Ruvuma","Mara","Kagera","Tabora","Lindi",
            "Summit","Harbor","Delta","Unity","Pioneer"]
suffixes = ["Traders Ltd","Supplies Co","Logistics","Enterprises","Services","Holdings","Engineering","Consultants",
            "Distributors","Motors","Printing","Foods","Tech Solutions","Construction","Security","Hardware","Pharma",
            "Energy","Media","Travel","Agro","Textiles","Transport","Stationers","Builders","Imports","Exports",
            "Catering","Cleaning","Electricals","Furniture","Telecom","Freight","Mining","Water","Oil & Gas",
            "Insurance Agency","Healthcare","Education","Hotels","Printers","Packaging","Plastics","Steel","Cement",
            "Paints","Tyres","Glass","Paper","Chemicals","Seeds","Fisheries","Dairy","Poultry","Timber","Solar",
            "Auto Parts","Office Mart","Events","Advertising","Audio Visual","Laboratories","Uniforms","Courier",
            "Garage","Machinery","Tools","Spares","Wholesale","Retail","Marketing","Legal","Audit","Payroll",
            "IT Services","Cabling","Fencing","Roofing","Plumbing","Painting","Landscaping","Pest Control",
            "Waste Mgmt","Car Hire","Fuel","Generators","Batteries","Lighting","Networks","Software","Hardware Hub",
            "Computers","Phones","Security Sys","Printing Hub","Signage","Branding","Photography","Video"]
vnames = np.array([f"{x} {y}" for x in prefixes for y in suffixes])
NV = len(vnames)
vendors = np.array([f"V{i:05d}" for i in range(1, NV + 1)])
accounts = [("5010","Office Supplies"),("5020","Travel & Accommodation"),("5030","Fuel & Transport"),
            ("5040","Repairs & Maintenance"),("5050","Professional Fees"),("5060","IT & Software"),
            ("5070","Utilities"),("5080","Rent"),("5090","Marketing & Advertising"),("5100","Training"),
            ("5110","Security Services"),("5120","Cleaning Services"),("5130","Printing & Stationery"),
            ("5140","Communication"),("5150","Insurance"),("1510","Motor Vehicles"),("1520","Computer Equipment"),
            ("1530","Furniture & Fittings"),("5160","Entertainment"),("5170","Donations")]
acc_code = np.array([c for c, _ in accounts]); acc_name = np.array([n for _, n in accounts])
depts = np.array(["Finance","HR","ICT","Operations","Procurement","Marketing","Audit","Legal","Admin","Sales"])
methods = np.array(["EFT","Cheque","Cash","Mobile Money","Card"])
approvers = np.array(["J. Mushi","A. Said","P. Lyimo","R. Massawe","H. Juma","G. Mwakyusa","F. Kimaro","S. Nyerere"])
statuses = np.array(["Paid","Paid","Paid","Paid","Pending","Approved","Rejected"])

# ---------------- generate columns ----------------
log(f"generating {N:,} rows")
step = np.ones(N, dtype=np.int64)
gap_idx = rng.choice(N, pct(0.003), replace=False)
step[gap_idx] += rng.integers(1, 5, len(gap_idx))
txn_ids = 100000 + np.cumsum(step); del step

start = dt.date(2023, 1, 1); ndays = (dt.date(2025, 12, 31) - start).days + 1
excel_base = (start - dt.date(1899, 12, 30)).days            # Excel serial of 2023-01-01
d = np.sort(rng.integers(0, ndays, N)).astype(np.int32)
txn_serial = (excel_base + d).astype(np.int32)
post_serial = (txn_serial + rng.integers(0, 6, N)).astype(np.int32); del d

v_idx = np.minimum(rng.zipf(1.3, N) - 1, NV - 1)
v_idx = ((v_idx * 7919 + rng.integers(0, 3, N)) % NV).astype(np.int32)
a_idx = rng.integers(0, len(accounts), N).astype(np.int8)
dep_idx = rng.integers(0, len(depts), N).astype(np.int8)
m_idx = rng.choice(len(methods), N, p=[.55, .15, .1, .15, .05]).astype(np.int8)
ap_idx = rng.integers(0, len(approvers), N).astype(np.int8)
st_idx = rng.integers(0, len(statuses), N).astype(np.int8)

amount = np.round(np.exp(rng.normal(12.5, 1.4, N)), 2)
round_idx = rng.choice(N, pct(0.008), replace=False);   amount[round_idx] = rng.integers(1, 50, len(round_idx)) * 1_000_000.0
outlier_idx = rng.choice(N, pct(0.0005), replace=False); amount[outlier_idx] = np.round(rng.uniform(5e8, 3e9, len(outlier_idx)), 2)
neg_idx = rng.choice(N, pct(0.0015), replace=False);    amount[neg_idx] *= -1
under_idx = rng.choice(N, pct(0.002), replace=False);   amount[under_idx] = rng.integers(4_900_000, 5_000_000, len(under_idx)).astype(float)

inv = rng.integers(10000, 9999999, N).astype(np.int32)
dup_idx = np.sort(rng.choice(np.arange(1000, N), pct(0.005), replace=False))
src = dup_idx - rng.integers(1, 800, len(dup_idx))
v_idx[dup_idx] = v_idx[src]; inv[dup_idx] = inv[src]; amount[dup_idx] = amount[src]

blank_desc = np.zeros(N, bool); blank_desc[rng.choice(N, pct(0.003), replace=False)] = True
blank_appr = np.zeros(N, bool); blank_appr[rng.choice(N, pct(0.0025), replace=False)] = True
weekend_count = int(np.sum((txn_serial + 5) % 7 >= 5))
log("generation done")

COLS = ["Txn_ID","Txn_Date","Posting_Date","Vendor_ID","Vendor_Name","Invoice_No","Account_Code",
        "Account_Name","Department","Description","Amount_TZS","Payment_Method","Approved_By","Status"]
README = [
    ("Test dataset", "Synthetic AP/payments data for audit-analytics testing. All names are fictional."),
    ("Rows", f"{N:,} transactions, 14 columns"),
    ("Date range", "2023-01-01 to 2025-12-31 (Txn_Date); Posting_Date = Txn_Date + 0-5 days"),
    ("Currency", "Amount_TZS in Tanzanian shillings"),
    ("Random seed", f"{a.seed} (reproducible)"),
    ("", ""),
    ("PLANTED ANOMALY", "EXPECTED COUNT / HOW TO FIND"),
    ("Gaps in Txn_ID sequence", f"{len(gap_idx):,} gap points (GAPS on Txn_ID)"),
    ("Duplicate payments", f"~{len(dup_idx):,} rows repeating Vendor_ID + Invoice_No + Amount_TZS of an earlier row (DUPLICATES)"),
    ("Round-million amounts", f"~{len(round_idx):,} rows with exact multiples of 1,000,000"),
    ("Large outliers", f"~{len(outlier_idx):,} rows between 500M and 3B TZS"),
    ("Negative amounts (reversals)", f"~{len(neg_idx):,} rows"),
    ("Just below 5M threshold", f"~{len(under_idx):,} rows between 4,900,000 and 4,999,999"),
    ("Blank Description", f"{int(blank_desc.sum()):,} rows"),
    ("Blank Approved_By", f"{int(blank_appr.sum()):,} rows"),
    ("Weekend transactions", f"{weekend_count:,} rows with Txn_Date on Sat/Sun"),
    ("Vendor concentration", "Vendor spend is deliberately skewed (a few vendors carry most rows) for STRATIFY/SUMMARIZE"),
    ("Note", "Counts marked ~ can differ slightly where anomalies overlap on the same row."),
]

# ---------------- Arrow chunks (CSV / Parquet) ----------------
def arrow_chunk(lo, hi):
    import pyarrow as pa
    unix = lambda s: pa.array(s.astype(np.int32) - 25569, pa.int32()).cast(pa.date32())  # Excel serial -> date
    ai, di = a_idx[lo:hi], dep_idx[lo:hi]
    desc = np.char.add(np.char.add(acc_name[ai], " - "), depts[di])
    return pa.table({
        "Txn_ID": pa.array(txn_ids[lo:hi]),
        "Txn_Date": unix(txn_serial[lo:hi]),
        "Posting_Date": unix(post_serial[lo:hi]),
        "Vendor_ID": pa.array(vendors[v_idx[lo:hi]]),
        "Vendor_Name": pa.array(vnames[v_idx[lo:hi]]),
        "Invoice_No": pa.array(np.char.add("INV-", inv[lo:hi].astype(str))),
        "Account_Code": pa.array(acc_code[ai]),
        "Account_Name": pa.array(acc_name[ai]),
        "Department": pa.array(depts[di]),
        "Description": pa.array(desc, mask=blank_desc[lo:hi]),
        "Amount_TZS": pa.array(amount[lo:hi]),
        "Payment_Method": pa.array(methods[m_idx[lo:hi]]),
        "Approved_By": pa.array(approvers[ap_idx[lo:hi]], mask=blank_appr[lo:hi]),
        "Status": pa.array(statuses[st_idx[lo:hi]]),
    })

CHUNK = 1_000_000

def write_parquet():
    import pyarrow.parquet as pq
    path = f"{a.out}.parquet"; w = None
    for lo in range(0, N, CHUNK):
        t = arrow_chunk(lo, min(lo + CHUNK, N))
        w = w or pq.ParquetWriter(path, t.schema, compression="zstd")
        w.write_table(t); log(f"parquet {min(lo+CHUNK, N):,}/{N:,}")
    w.close(); log(f"wrote {path}")

def write_csv():
    import pyarrow.csv as pcsv
    path = f"{a.out}.csv"; w = None
    for lo in range(0, N, CHUNK):
        t = arrow_chunk(lo, min(lo + CHUNK, N))
        w = w or pcsv.CSVWriter(path, t.schema)
        w.write_table(t); log(f"csv {min(lo+CHUNK, N):,}/{N:,}")
    w.close(); log(f"wrote {path}")

def write_readme_txt():
    with open(f"{a.out}_README.txt", "w") as f:
        for k, v in README: f.write(f"{k:32} {v}\n" if k else "\n")

# ---------------- Excel ----------------
TMP_BYTES_PER_ROW = 1600      # uncompressed sheet XML xlsxwriter holds in temp files until close()
XLSX_BYTES_PER_ROW = 90       # final compressed .xlsx

def check_space(rows_at_once, label):
    need = rows_at_once * TMP_BYTES_PER_ROW + rows_at_once * XLSX_BYTES_PER_ROW
    free = shutil.disk_usage(tmpdir).free
    log(f"{label}: needs ~{need/1e9:.1f} GB temp space in {tmpdir} (free: {free/1e9:.1f} GB)")
    if free < need * 1.1:
        raise SystemExit(f"Not enough disk space in {tmpdir}. Free up space, pass --tmpdir <folder on a bigger disk>, "
                         f"or use --split-files.")

def write_xlsx():
    import xlsxwriter
    vl, vnl = vendors.tolist(), vnames.tolist()
    acl, anl, dl = acc_code.tolist(), acc_name.tolist(), depts.tolist()
    ml, al, sl = methods.tolist(), approvers.tolist(), statuses.tolist()
    widths = [11,12,12,10,30,12,12,24,13,34,16,15,14,10]
    chunks = [(s * a.sheet_rows, min((s + 1) * a.sheet_rows, N)) for s in range(-(-N // a.sheet_rows))]
    nch = len(chunks)

    def new_workbook(path, layout_line):
        wb = xlsxwriter.Workbook(path, {"constant_memory": True, "use_zip64": True, "tmpdir": tmpdir})
        f = dict(hdr=wb.add_format({"bold": True, "font_name": "Arial", "bg_color": "#1F3864", "font_color": "white", "border": 1}),
                 date=wb.add_format({"num_format": "yyyy-mm-dd", "font_name": "Arial"}),
                 num=wb.add_format({"num_format": "#,##0.00", "font_name": "Arial"}),
                 txt=wb.add_format({"font_name": "Arial"}))
        f["layout"] = layout_line
        return wb, f


    def write_sheet(wb, f, name, lo, hi):
        txt, datef, numf = f["txt"], f["date"], f["num"]
        ws = wb.add_worksheet(name)
        for i, w in enumerate(widths): ws.set_column(i, i, w)
        ws.freeze_panes(1, 0); ws.write_row(0, 0, COLS, f["hdr"])
        ids, ts, ps = txn_ids[lo:hi].tolist(), txn_serial[lo:hi].tolist(), post_serial[lo:hi].tolist()
        vs, invs, amts = v_idx[lo:hi].tolist(), inv[lo:hi].tolist(), amount[lo:hi].tolist()
        ais, dis, mis = a_idx[lo:hi].tolist(), dep_idx[lo:hi].tolist(), m_idx[lo:hi].tolist()
        aps, sts = ap_idx[lo:hi].tolist(), st_idx[lo:hi].tolist()
        bd, ba = blank_desc[lo:hi].tolist(), blank_appr[lo:hi].tolist()
        for r in range(hi - lo):
            R = r + 1; vi = vs[r]; ai = ais[r]; di = dis[r]
            ws.write_number(R, 0, ids[r], txt)
            ws.write_number(R, 1, ts[r], datef)
            ws.write_number(R, 2, ps[r], datef)
            ws.write_string(R, 3, vl[vi], txt)
            ws.write_string(R, 4, vnl[vi], txt)
            ws.write_string(R, 5, f"INV-{invs[r]}", txt)
            ws.write_string(R, 6, acl[ai], txt)
            ws.write_string(R, 7, anl[ai], txt)
            ws.write_string(R, 8, dl[di], txt)
            if not bd[r]: ws.write_string(R, 9, f"{anl[ai]} - {dl[di]}", txt)
            ws.write_number(R, 10, amts[r], numf)
            ws.write_string(R, 11, ml[mis[r]], txt)
            if not ba[r]: ws.write_string(R, 12, al[aps[r]], txt)
            ws.write_string(R, 13, sl[sts[r]], txt)

    if not a.split_files or nch == 1:
        check_space(N, "single workbook")
        path = f"{a.out}.xlsx"
        wb, f = new_workbook(path, f"{nch} sheets: Transactions_01 .. Transactions_{nch:02d}, "
                                   f"{a.sheet_rows:,} rows each (last may be smaller)")
        try:
            for s, (lo, hi) in enumerate(chunks):
                write_sheet(wb, f, f"Transactions_{s+1:02d}", lo, hi)
                log(f"xlsx sheet {s+1}/{nch} written ({hi:,}/{N:,} rows)")
            log("compressing xlsx (can take a few minutes)...")
        finally:
            wb.close()          # also removes xlsxwriter's temp files, even on error
        log(f"wrote {path}")
    else:
        check_space(chunks[0][1] - chunks[0][0], "per workbook")
        for s, (lo, hi) in enumerate(chunks):
            path = f"{a.out}_{s+1:02d}.xlsx"
            wb, f = new_workbook(path, f"File {s+1} of {nch} ({a.out}_01.xlsx .. {a.out}_{nch:02d}.xlsx). "
                                       f"This file: rows {lo+1:,}-{hi:,}. README counts cover ALL files.")
            try:
                write_sheet(wb, f, "Transactions", lo, hi)
            finally:
                wb.close()
            log(f"wrote {path} ({hi:,}/{N:,} rows, {os.path.getsize(path)/1e6:.0f} MB)")

if a.format in ("parquet", "all"): write_parquet()
if a.format in ("csv", "all"): write_csv()
if a.format in ("xlsx", "all"): write_xlsx()
print()
for k, v in README[6:]: print(f"{k:32} {v}")
log("done")
