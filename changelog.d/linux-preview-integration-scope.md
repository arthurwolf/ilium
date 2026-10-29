### Fixed
- Select Linux preview product integration targets from Cargo metadata so plugin-preview suites embedded in product crates do not block the explicitly plugin-free early release; retain all other product tests and unchanged full-release qualification.
- Retain the preview test selection and its Cargo metadata in the uploaded, hash-bound native evidence.
