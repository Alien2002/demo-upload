import numpy as np, xlsxwriter, datetime as dt, time

N = 1_000_000
rng = np.random.default_rng(42)
t0 = time.time()

# --- reference data ---
vendors = [f"V{i:05d}" for i in range(1, 2501)]
vnames = [f"{a} {b}" for a in ["Kilimanjaro","Serengeti","Zanzibar","Mwanza","Arusha","Dodoma","Tanga","Mbeya","Morogoro","Iringa",
                                "Coastal","Lake","Savanna","Baobab","Pwani","Ruvuma","Mara","Kagera","Tabora","Lindi",
                                "Summit","Harbor","Delta","Unity","Pioneer"]
                       for b in ["Traders Ltd","Supplies Co","Logistics","Enterprises","Services","Holdings",
                                 "Engineering","Consultants","Distributors","Motors","Printing","Foods",
                                 "Tech Solutions","Construction","Security","Hardware","Pharma","Energy",
                                 "Media","Travel","Agro","Textiles","Transport","Stationers","Builders",
                                 "Imports","Exports","Catering","Cleaning","Electricals","Furniture",
                                 "Telecom","Freight","Mining","Water","Oil & Gas","Insurance Agency",
                                 "Healthcare","Education","Hotels","Printers","Packaging","Plastics",
                                 "Steel","Cement","Paints","Tyres","Glass","Paper","Chemicals",
                                 "Seeds","Fisheries","Dairy","Poultry","Timber","Solar","Auto Parts",
                                 "Office Mart","Events","Advertising","Audio Visual","Laboratories",
                                 "Uniforms","Courier","Garage","Machinery","Tools","Spares","Wholesale",
                                 "Retail","Marketing","Legal","Audit","Payroll","IT Services","Cabling",
                                 "Fencing","Roofing","Plumbing","Painting","Landscaping","Pest Control",
                                 "Waste Mgmt","Car Hire","Fuel","Generators","Batteries","Lighting",
                                 "Networks","Software","Hardware Hub","Computers","Phones","Security Sys",
                                 "Printing Hub","Signage","Branding","Photography","Video"]]
NV = len(vnames); vendors = vendors[:NV]
accounts = [("5010","Office Supplies"),("5020","Travel & Accommodation"),("5030","Fuel & Transport"),
            ("5040","Repairs & Maintenance"),("5050","Professional Fees"),("5060","IT & Software"),
            ("5070","Utilities"),("5080","Rent"),("5090","Marketing & Advertising"),("5100","Training"),
            ("5110","Security Services"),("5120","Cleaning Services"),("5130","Printing & Stationery"),
            ("5140","Communication"),("5150","Insurance"),("1510","Motor Vehicles"),("1520","Computer Equipment"),
            ("1530","Furniture & Fittings"),("5160","Entertainment"),("5170","Donations")]
depts = ["Finance","HR","ICT","Operations","Procurement","Marketing","Audit","Legal","Admin","Sales"]
methods = ["EFT","Cheque","Cash","Mobile Money","Card"]
approvers = ["J. Mushi","A. Said","P. Lyimo","R. Massawe","H. Juma","G. Mwakyusa","F. Kimaro","S. Nyerere"]
statuses = ["Paid","Paid","Paid","Paid","Pending","Approved","Rejected"]

# --- sequential IDs with ~0.3% gaps ---
step = np.ones(N, dtype=np.int64); gap_idx = rng.choice(N, 3000, replace=False)
step[gap_idx] += rng.integers(1, 5, 3000)
txn_ids = 100000 + np.cumsum(step)

# --- dates 2023-01-01 .. 2025-12-31, sorted with txn id ---
start = dt.date(2023, 1, 1); days = (dt.date(2025, 12, 31) - start).days + 1
d = np.sort(rng.integers(0, days, N))
excel_base = (start - dt.date(1899, 12, 30)).days
txn_date = excel_base + d
post_date = txn_date + rng.integers(0, 6, N)

v_idx = np.minimum(rng.zipf(1.3, N) - 1, NV - 1)          # skewed: few vendors dominate
v_idx = (v_idx * 7919 + rng.integers(0, 3, N)) % NV
a_idx = rng.integers(0, len(accounts), N)
dep_idx = rng.integers(0, len(depts), N)
m_idx = rng.choice(len(methods), N, p=[.55, .15, .1, .15, .05])
ap_idx = rng.integers(0, len(approvers), N)
st_idx = rng.integers(0, len(statuses), N)

amount = np.round(np.exp(rng.normal(12.5, 1.4, N)), 2)   # TZS, median ~270k
# anomalies
round_idx = rng.choice(N, 8000, replace=False); amount[round_idx] = rng.integers(1, 50, 8000) * 1_000_000.0
outlier_idx = rng.choice(N, 500, replace=False); amount[outlier_idx] = np.round(rng.uniform(5e8, 3e9, 500), 2)
neg_idx = rng.choice(N, 1500, replace=False); amount[neg_idx] *= -1           # credit notes/reversals
under_idx = rng.choice(N, 2000, replace=False); amount[under_idx] = rng.integers(4_900_000, 5_000_000, 2000).astype(float)  # just under 5M threshold

inv = rng.integers(10000, 9999999, N)
# duplicate payments: copy vendor+invoice+amount from an earlier row
dup_idx = np.sort(rng.choice(np.arange(1000, N), 5000, replace=False))
src = dup_idx - rng.integers(1, 800, 5000)
v_idx[dup_idx] = v_idx[src]; inv[dup_idx] = inv[src]; amount[dup_idx] = amount[src]

blank_desc = set(rng.choice(N, 3000, replace=False).tolist())
blank_appr = set(rng.choice(N, 2500, replace=False).tolist())

wd = (txn_date + 5) % 7   # 0=Mon ... 5=Sat 6=Sun (Excel serial 1 = Sunday 1900-01-01)
weekend_count = int(np.sum(wd >= 5))

# --- write ---
wb = xlsxwriter.Workbook("test_transactions_1M.xlsx",
                         {"constant_memory": True, "strings_to_numbers": False})
hdr = wb.add_format({"bold": True, "font_name": "Arial", "bg_color": "#1F3864", "font_color": "white", "border": 1})
datef = wb.add_format({"num_format": "yyyy-mm-dd", "font_name": "Arial"})
numf = wb.add_format({"num_format": "#,##0.00", "font_name": "Arial"})
txt = wb.add_format({"font_name": "Arial"})

ws = wb.add_worksheet("Transactions")
cols = ["Txn_ID","Txn_Date","Posting_Date","Vendor_ID","Vendor_Name","Invoice_No","Account_Code",
        "Account_Name","Department","Description","Amount_TZS","Payment_Method","Approved_By","Status"]
widths = [11,12,12,10,30,12,12,24,13,34,16,15,14,10]
for i, w in enumerate(widths): ws.set_column(i, i, w)
ws.freeze_panes(1, 0)
ws.write_row(0, 0, cols, hdr)

for r in range(N):
    R = r + 1
    vi = int(v_idx[r]); ac = accounts[a_idx[r]]
    ws.write_number(R, 0, int(txn_ids[r]), txt)
    ws.write_number(R, 1, int(txn_date[r]), datef)
    ws.write_number(R, 2, int(post_date[r]), datef)
    ws.write_string(R, 3, vendors[vi], txt)
    ws.write_string(R, 4, vnames[vi], txt)
    ws.write_string(R, 5, f"INV-{inv[r]}", txt)
    ws.write_string(R, 6, ac[0], txt)
    ws.write_string(R, 7, ac[1], txt)
    ws.write_string(R, 8, depts[dep_idx[r]], txt)
    if r not in blank_desc:
        ws.write_string(R, 9, f"{ac[1]} - {depts[dep_idx[r]]}", txt)
    ws.write_number(R, 10, float(amount[r]), numf)
    ws.write_string(R, 11, methods[m_idx[r]], txt)
    if r not in blank_appr:
        ws.write_string(R, 12, approvers[ap_idx[r]], txt)
    ws.write_string(R, 13, statuses[st_idx[r]], txt)
    if r % 200000 == 0: print(r, round(time.time() - t0), flush=True)

# README sheet with planted anomalies
rd = wb.add_worksheet("README")
rd.set_column(0, 0, 40); rd.set_column(1, 1, 70)
rows = [("Test dataset", "Synthetic AP/payments data for audit-analytics testing. All names are fictional."),
        ("Rows", f"{N:,} transactions (sheet 'Transactions'), 14 columns"),
        ("Date range", "2023-01-01 to 2025-12-31 (Txn_Date); Posting_Date = Txn_Date + 0-5 days"),
        ("Currency", "Amount_TZS in Tanzanian shillings"),
        ("Random seed", "42 (reproducible)"),
        ("", ""),
        ("PLANTED ANOMALY", "EXPECTED COUNT / HOW TO FIND"),
        ("Gaps in Txn_ID sequence", f"{len(gap_idx):,} gap points (GAPS on Txn_ID)"),
        ("Duplicate payments", f"~{len(dup_idx):,} rows repeating Vendor_ID + Invoice_No + Amount_TZS of an earlier row (DUPLICATES)"),
        ("Round-million amounts", f"~{len(round_idx):,} rows with exact multiples of 1,000,000"),
        ("Large outliers", f"~{len(outlier_idx):,} rows between 500M and 3B TZS"),
        ("Negative amounts (reversals)", f"~{len(neg_idx):,} rows"),
        ("Just below 5M threshold", f"~{len(under_idx):,} rows between 4,900,000 and 4,999,999"),
        ("Blank Description", f"{len(blank_desc):,} rows"),
        ("Blank Approved_By", f"{len(blank_appr):,} rows"),
        ("Weekend transactions", f"{weekend_count:,} rows with Txn_Date on Sat/Sun"),
        ("Vendor concentration", "Vendor spend is deliberately skewed (a few vendors carry most rows) for STRATIFY/SUMMARIZE"),
        ("Note", "Counts marked ~ can differ slightly where anomalies overlap on the same row.")]
for i, (a, b) in enumerate(rows):
    fmt = hdr if a in ("PLANTED ANOMALY",) else txt
    rd.write_string(i, 0, a, fmt); rd.write_string(i, 1, b, fmt)

wb.close()
print("done", round(time.time() - t0), "s")
