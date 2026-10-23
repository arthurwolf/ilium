# API gaps of the skel-api lane

- `ilium-plugin-api::version::final_minor_loadable`: a date-aware retention check (ABI-950, ABI-951): given a package `api` (`ApiVersion`), the major release that succeeded its line, and a check date, it says whether the final minor of that major line still loads through its frozen copy and adapter (true until 12 months after the successor release, false after). The tests cover the one-day-before boundary for the 0.x line (ABI-950) and the 1.x and 2.x lines (ABI-951); the item does not exist yet.
- `ilium-plugin-api::version::retention_refusal`: the refusal side of the same window (ABI-950, ABI-951): after the 12-month date a package of the retired major line gets exactly `package.api-unsupported` (`codes::package::API_UNSUPPORTED`) and no other code; the item does not exist yet.
- The 12-month window is marked "proposed, owner to confirm" in the manual, so the exact day-count boundary is an assumption of these tests until the owner confirms it.
