---
name: reference_tmo_los_attachment_document_api
description: The Mortgage Office (TMO) LOS.svc attachment/document API contract + how to fetch the Postman collection behind developers.themortgageoffice.com
metadata: 
  node_type: memory
  type: reference
  originSessionId: 6764f3ee-e720-44c8-ab4c-b986139645c6
---

TMO document ("attachment") API used for EFS Loan Package ↔ TMO doc sync (#tmo-doc-sync).
Same envelope as the rest of the TMO client (`app/equityfundingsource/lib/tmo-api/index.ts`):
base `https://api.themortgageoffice.com`, path `/LOS.svc/<Endpoint>`, headers
`Token` + `Database`, response `{ Data, ErrorMessage, ErrorNumber, Status }` (Status 0 / ErrorNumber 0 = OK).

**Loan Origination (LOS.svc) attachment endpoints** — this is what EFS uses:
- `GET /LOS.svc/GetLoanAttachments/:LoanNumber` — list metadata only (no content). `Data` = array of
  `{ RecID, FileName, Description, DocType, SysCreatedDate, Tab/TabName }`. Loan-not-found returns 400/Status -1 (treat as empty).
- `GET /LOS.svc/GetLoanAttachment/:RecId` — one attachment WITH file content. Same shape; content field.
- `POST /LOS.svc/AddAttachment/:LoanNumber` — body `{ FileName, Description, TabName, DocType, Content }` where
  `Content` is **base64**. `DocType: -1` = default/unknown. Returns `Data` = the new attachment RecID string. Max 2GB.

**Content field is ambiguous — decode defensively.** Documented Response Structure table names the download
field `Content` (base64 string), but the live DataContract payload (`__type: CLOSAttachment:#TmoAPI`) uses
`Document` and aliases `Tab`(→TabName)/`OwnerRecID`(→RecID). .NET may serialize byte[] as a base64 string OR a
JSON array of byte ints. EFS reads `Content` then `Document` then `FileContent`/`Data`; string→base64-decode,
number[]→Uint8Array. There is also a parallel Loan Servicing surface (`LSS.svc/GetLoanAttachments/:Account`,
`LSS.svc/AddLSAttachment/:LoanNumber`, `LSS.svc/Attachment/:RecID`) — EFS deliberately uses LOS.svc for consistency.

**How to get the full contract** (the docs site is a JS-rendered Postman SPA — WebFetch/firecrawl return only "TMO API"):
```
curl -sL https://developers.themortgageoffice.com/ | grep -o 'collections/[0-9]*/[A-Za-z0-9]*'   # → 37774064/2sAXjGcE4E
curl -sL https://documenter.gw.postman.com/api/collections/37774064/2sAXjGcE4E -o tmo.json         # full collection JSON (~4.7MB)
jq -r '.item[].name' tmo.json                    # folders: Loan Origination / Loan Servicing / Mortgage Pools
# attachment requests live under Loan Origination > "Loan Attachments"
```
Strip the huge base64 `Content` blobs when jq-printing request/response bodies.
See [[reference_efs_d1_token_working]], [[reference_efs_verify_deploy_via_string_grep]].
