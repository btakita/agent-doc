# Portal QA Review — Client Feedback Request

---

## 1. Borrower New Loan Application Onboarding Page
**URL:** `/portal-borrower/new-application`

Items to review:

1. **Forms for each onboarding step** *(High priority)*
   Please confirm that the form fields and flow for each step are correct.

2. **Document submission**
   How many documents is the borrower required to submit during the onboarding process?

3. **Document storage**
   Uploaded files are stored in Dropbox using the following path structure:
   ```
   /loan_applications/{loan_number}/{file_name}.{file_type}
   ```
   Please confirm this is the expected behavior.

---

## 2. Borrower Application Status Page
**URL:** `/portal-borrower/application-status`

I'd like to clarify the distinction between **Loan Status** and **Progress**, as they serve different purposes:

- **Loan Status** has three values: *Open*, *Closed*, *Unassigned*.
- **Progress** refers to the six-step review process completed by an admin (as shown in the original design). Once all six steps are confirmed by the admin, the Loan Status is updated to *Open*.

Could you confirm whether this logic is correct? Additionally, could you clarify what each of the three Loan Status values (*Open*, *Closed*, *Unassigned*) represents in the TMO service?

---

## 3. Admin Loan Application Management Page
**URL:** `/portal-admin/loans`

1. **Table comparison**
   The table on this page is very similar to the one on the borrower's Application Status page. I'd love to hear your thoughts on whether any differentiation is needed between the two, or if the current approach makes sense.

2. **Loan Progress Modal**
   Clicking the edit icon opens a modal where the admin can update the loan progress step by step. The admin also has the option to enter a reason when denying a specific step. Does this interaction feel intuitive and appropriate for the workflow?

---

## Test Credentials

| Role     | Email                  | Password         |
|----------|------------------------|------------------|
| Borrower | test1@borrower.com     | test!@#$1234     |
| Admin    | btak.dev@gmail.com     | test!@#$1234     |

**Admin Sign-In URL:** `/admin/login`
*(Also accessible via the link at the bottom of any sign-in page)*

**Admin Signup Global Password:** `equityfundingsource!@#$1234`
