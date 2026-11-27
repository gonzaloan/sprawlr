# See the account the way an AWS architecture diagram draws it

Status: Built · Owner: gonzaloan · Date: 2026-11-27

## Problem

The map answers "who calls whom", laid out by connections. Someone who wants to know where things run (which region, which VPC, which subnets and availability zones, what sits outside any VPC) has to open resources one by one. Nothing shows at a glance whether a service is spread over two availability zones or lives in one, or what sits in public subnets. Teams draw that picture by hand in a diagram tool, and it goes stale.

## Outcome

A second way to look at any place in sprawlr, next to Map and Documentation: the same resources, drawn where they run. In one screen a person can tell:
- what enters from the internet and through which edge services;
- which VPCs exist, their availability zones and subnet tiers, and what runs in each;
- which availability zones each resource has subnets in;
- what runs outside any VPC, grouped by what it does;
- the health of each piece, and how a selected piece connects to the rest.

## Scope

**In:**
- An "Architecture" tab for the whole account, an app, a flow and a resource. The tab is part of the URL, so Back, Forward and links keep it.
- Layout: people on the internet, then edge services (CloudFront, Route 53), then the account and each region. Inside a region come its VPCs, and after them the services outside any VPC, grouped by what they do. IAM roles go in an account-wide group. Services outside AWS go last.
- In a VPC: one column per availability zone and one row per subnet tier. A tier is public or private when the subnet's name says so (`prod-public-a`, `PrivateSubnet1`); a subnet that gives new instances public IPs counts as public; any other subnet is just "Subnets". Each tier says how it was read. A resource spans the zones of its subnets, once per run of adjacent zones. NAT gateways sit in their subnets, drawn as shared plumbing.
- Availability-zone notes, as configured: "Multi-AZ" or "Single-AZ" (a database setting), "Subnets in 2 AZs", and "Subnets in 1 AZ" for something that can span zones, in a VPC that has more than one. How many zones something actually runs in also depends on how many copies run; judging it is left to findings.
- On every resource: its health from CloudWatch (alarms and metrics already read), and its findings.
- Connections of the selected or hovered resource, drawn with the same evidence styles as the map, and a switch to draw all of them.
- A VPC with nothing of this place in it is shown on the whole-account view as one line ("default VPC: 6 subnets in 6 zones, nothing runs in it", or "4 resources, none in prod" when something runs in another environment), and left out elsewhere.
- The resource panel names the availability zones a resource runs in.

**Out (and why):**
- AWS Architecture Icons. Their terms cover diagrams and documents; they don't say whether the icons can ship inside software, and sprawlr is distributed. Each kind keeps sprawlr's own mark, coloured by what it does.
- CloudTrail ("who changed what"). It is a new read whose events name people (user names, session names, source addresses). It needs its own spec: what to keep, how to mask it, and an opt-in.
- Route tables and security group rules drawn as lines. Subnet tiers come from names and auto-assigned public IPs. Route tables can come later, with the reads they need.
- Editing the layout by hand. The view is generated, like the docs.

## Experience

1. The person opens an app and picks "Architecture" next to "Map". The checkout app shows people on the internet, then the CloudFront distribution, then us-east-1. Inside us-east-1, prod-vpc has two columns, us-east-1a and us-east-1b, and two rows:
   - public: checkout-alb, across both columns, marked "Subnets in 2 AZs";
   - private: checkout-api, across both, and reports-worker, in us-east-1a.
   
   checkout-orders-db sits in the VPC with "Multi-AZ". After the VPC come the services outside it: SQS and SNS under "Messaging and events", the secret under "Keys and secrets".
2. Hovering checkout-api draws its connections. Clicking it opens the resource panel, as on the map.
3. On the whole account, every region and VPC appears. The default VPC, with nothing in it, takes one line.
4. A serverless app (no VPC) shows its region with its groups and no VPC box.

Empty state: when the place has no resources (an environment with nothing in it), the tab says so and how to move on, as the map does.

## Design

- **Domain (frontend, `app/src/domain/architecture.ts`):** `architectureOf(ix, view, options)` returns zones (users, edge, account-wide, regions, outside). A region holds VPC plans and service groups. A VPC plan holds its zones, its tiers and the resources placed in them with a column span, plus what has no subnet. Also `subnetTier` and `zonesOf`. It is pure view logic over the snapshot: placement, not new facts about the account. No new rules in Rust.
- **Views:** `everythingView(ix, env, permissions)` gives the whole account as resources, for this tab. The map's overview keeps drawing apps.
- **UI (`app/src/features/architecture`):** CSS grid for the zones and tiers. An SVG layer measures the resources it connects and draws the lines, again when the size changes.
- **Location:** `Tab` gains `"architecture"`.
- **Snapshot contract:** unchanged.
- **Costs, permissions, privacy:** no new AWS call, no new permission, nothing new stored.

## Acceptance criteria

- [x] Given the demo's checkout app, when Architecture is open, then checkout-alb spans us-east-1a and us-east-1b in public subnets, checkout-api spans both in private subnets, and checkout-orders-db is marked Multi-AZ.
- [x] Given a subnet named `prod-public-a`, then its tier is public, read from its name; given one with no hint in its name that assigns public IPs, then public, read from that; given neither, then "Subnets", tier not known.
- [x] Given an app with no VPC (billing), then the region shows its services by group and no VPC box.
- [x] Given a VPC with nothing of the place in it, then the whole account shows it as one line, and an app doesn't show it.
- [x] Given a selected resource, then its connections are drawn with their evidence; with "All connections", every connection between drawn resources is.
- [x] Given `#/app/checkout?tab=architecture`, when the link is opened, then the Architecture tab is on, and Back returns to the Map.
- [x] Given a resource in two subnets, when its panel opens, then it names both zones.

## Open questions

- Route tables would turn subnet tiers from a reading of names into a fact. Worth reading in a later scan change.
