# Separate service packages

XKK is a Cargo workspace with independent `logic`, `gate`, `public`, and `query` packages because their listeners, dependencies, and startup configuration differ materially. Each package composes `xframe` directly; a shared bootstrap package will be added only when repeated application behavior exists beyond what `xframe` already provides.
