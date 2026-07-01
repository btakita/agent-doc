Commercial Real Estate Marketplace

Automated Digital Marketing and Transaction Management Platform

V1.1 – Beta Soft Launch 10/14/2025

PLATFORM DEMO

https://www.youtube.com/watch?v=1Q8RdnQwYLw

V1.2

CURRENT TECH STACK

Frontend: Vue.js

Backend: Rust

Hosting: Google Cloud

Database: Google Cloud SQL / PostgreSQL

Authentication: Authenticator with MFA

Security: HTTPS, SSL, IAM, Zero-Trust access control

Infrastructure: Terraform

Database Migrations: Redgate Flyway

Monitoring: Google Cloud Monitoring, profiler, metrics, dashboards

Backend Modules: Services, Activities, Network, Notifications, Listings

Development Requirements: 3 Core Workstreams

1. Admin DBMS Enhancements
2. Broker Segment Creation
3. Listing Creation & OM Rebuild

1. Admin DBMS Enhancements

Enhance the existing admin/database management system inside the platform into a more
robust database management and admin operations system. The platform already includes many
of the core tools for managing users, contacts, companies, subscriptions, activity, and outreach,
but these tools need to be expanded, improved, and made more flexible with additional fields,
filters, editing options, tracking capabilities, and admin controls.

The goal is to build on the current system, not replace it, so admins can more efficiently
manage and update platform records, monitor user activity, manage email/subscription status,
maintain accurate data, and run basic outreach actions from one centralized admin area.

CURRENT ADMIN DASHBOARD

Main Work Items

1.1-  Listings Management Enhancements

•  Add the option to delete listing from action drop down. This deletes all data in relation

to that listing.

•  Option to click and view NDAs and LOIs signed on each listing

LISTING MANAGEMENT W/ ACTIONS & FILTER BY STATUS

1.2-  User Management Enhancements

Summary table should include the following Column Headers (add missing
& remove non-listed):

Name | Title | Company | Client Status (Multiple choice) | Email | Status | Last Update (text
field) | Last Contacted (text field) | LinkedIn | State | Asset Classes (Option to select
multiple) | Regions (Option to select multiple) | Minimum Deal Size (multiple choice) |
Maximum Deal Size (multiple coice)| Created Date | Email Status (mailerlite) | Newsletter
(Subscribed or Unsubscribed – pull from mailerlite) | Investments (Subscribed or Unsubscribed
– pull from different mailerlite account, used to send the property teaser when there is a match)

Admin functionality from summary table and profile page should include:

•  Click to view and edit complete user profile pages. Each user profile should include all

the info/data from the summary table as well as User Analytics listed below

•  Track email status as Active or Not Active/bounced (integration from mailerlite)
•  Search/filter users by all the different data points we have on them.
•  Allow to edit user info directly on summary table, without having to click from drop

down and without the pop up.

•  View up to 50 contacts per page on summary table.
•  Send bulk platform invitations to selected users from user management summary table
•  Send individual property teasers/flyers to selected users from user management

summary table. Currently this can be sent via the listings management page, under
actions. From there, it can only be sent to all the matches in network based on pre-set
acquisition criteria, or to one email as a test. We need to be able to select a certain
number of users to send it to.

•  Bulk import and export users with audit trail from the user management summary table.
•  Remove the Invite network button (was used for beta soft launch to invite entire

network, no longer needed)

•  Create a new button for all the new bulk actions mentioned above for selected users.
•  Add option to delete users both individually and in bulk/selected.

USER MANAGEMENT W/ ACTIONS DROPDOWN (ONLY 1 – EDIT)

Analytics – Under User Management Profile Pages

User / Platform Activity Tracking Enhancements

Improve activity tracking so admins can see key platform engagement and account activity
from the relevant user/contact profile.
Track activity such as:
•  Last signed in
•  NDAs signed through platform

•  LOIs submitted through platform
•  Property teaser engagement (from mailerlite)
•  History of messages sent through platform

1.3-  Company Management Enhancements

Summary table should include the following Column Headers (add missing
& remove non-listed):

Company: Name | Type | Assets Under Management | Contacts Count (number of contacts
we have from that company) | Website | LinkedIn URL | Status | City (text field) | State
(multiple choices) | Created Date

•  Click to view and edit full company profiles. Each company profile should include all

the info/data we have on them.

•  View which users/contacts belong to each company. (every user is under a company in

our network)

•  Search/filter companies by relevant data points.
•  View up to 50 companies per page.
•  Bulk import and export companies with audit trail from the summary table.
•  Option to delete company both individually and in bulk/selected.
•  Create a new button for all the new bulk actions mentioned above for selected users.

COMPANY  MANAGEMENT W/ ACTIONS DROPDOWN (ONLY 1 – EDIT)

2. Broker Segment Creation

Create a broker-specific account system with an approval workflow, separate broker database,
and broker-only listing management tools.

Brokers must be able to apply for access, be approved before entering the system, and then create
and manage listings under broker-specific permissions/pages.

Broker and buyer networks must remain fully separate, with different capabilities, requirements,
and access controls.

The platform must also remove broker-irrelevant features from dashboard, acquisitions, browse,
and profile areas to reflect the new workflow for broker accounts.

Main Work Items

2.1- Broker Database

•  Create a separate broker database management system under the admin pages.

BROKER User management summary table should ONLY include: Full name | Title | |
Email | Company | License # | State | LinkedIn | Activated | Created Date | Email Status
(Mailerlite).

BROKER Company management summary table should ONLY include: Name | License # |
State | Website | LinkedIn | Contact Count

2.2- Approval Workflow

•  Brokers can apply for Free (must show relevant CRE transaction experience), must be

approved. Once approved, it’s free to log in and create a listing.

•  Create a broker application flow for users requesting broker account access
•  Brokers must be approved before entering the platform.

2.3- Broker Pages

•  Broker should have access to different pages than principals. Below are the changes

needed:

1.  Remove “User Acquisition Criteria” box from the Acquisitions Page. Keep others

for listings added to profile via the code.

2.  Remove the “Browse” page. Broker profiles should not have access to the other

listings on the platform, only with the property code.

3.  Remove Browse Listings, Acquisition Criteria, and Request BOV buttons from

Dashboard page

4.  Update dashboard so Engaged Listings shows listings a broker accessed by

property code and signed an NDA for

5.  Update dashboard so Saved Listings includes listings once accessed via property

code

6.  Remove “Company Peers” from profile pages
7.  Remove Company Activities box from profile pages

•  Create a code for each listing on the platform. Through that code, any user can access the

listing once logged into the platform, including brokers.

•  Add a “Broker” tag to broker listings and a “Principal” tag to owner-direct listings so

buyers can distinguish listing type

3. Listing Creation & OM Rebuild

Listing creation / edit being reimplemented in Rust / Tera: Here are some of the most important
features the current version supports. The implementation should exactly match the current UI /
visuals, with the tera templates and backend changes being more modular and consolidated than
the current version. Apart from keeping the current UX, some of the core features supported
include (to be complemented afterwards if needed):

•  On creation: the steps should only be unlocked once the user filled in all the required
fields for that step, otherwise the "locked" state should be present for all the following
steps (the user is only allowed to proceed to the immediate next step, and once the current
step required fields are completed the next step only should be unlocked. Also, when the
user created a new listing, he can only move the listing to "Under Review" status, by
submitting the listing for review by Reitrades team (which needs to approve it for it to get
listed). After listing approval, the seller can change the status as he wants;

•  Each asset class allows for their specific set of required / optional attributes, and each

step also allows for custom attributes specified by the user;

•  The teaser email and the OM data derives from the listing data, so make sure both work
properly for each asset class if data store changes are needed. Also, if data store changes
are needed, we will need some sort of migration script from the current data model to the
new one given we already have listings in production;

•  The Marketing Materials derive from both Batchdata and AI generated fields (fallback)
and they should be automatically generated once, or when the user clicks the re-generate
button;

Please implement one asset class at a time, so that we can review the PRs and make sure the new
code structure is solid. We will review this task very closely, so please make sure to structure it
the better way possible. Divide and conquer (dividing the code in modules) is the preferred
approach.

Additional Comments/Requests

1.  AI generated content needs to be much better overall, including demographics and

the data to be 100% accurate

2.  OM generator needs to be polished/touched-up so the OMs come out more

professional.

3.  Ensure the additional attributes are being added to the OM being generated.
4.  Create function to allow brokers (only) to upload and their own OM (if uploaded

through files, it’s not set up to go via email when NDA is signed). Principals must
use our OM generator.

5.  Allow all users to upload Old OMs/property flyers and our system pulls all the

information and auto fills the information required to create a listing and generate
the OM

6.  Create a 3 tier option for when creating a listing to both principals and brokers.
This should be the first step/question:1: Free/basic (platform listing only); 2:
Premium $199 (platform listing + network email); 3: Elite $499 (we do
everything, they are added as contributors). Payment through stripe links.

LISTING CREATION STEPS


