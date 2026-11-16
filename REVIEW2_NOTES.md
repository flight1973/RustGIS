# Review round 2 notes (removed in the last commit)

- [x] 6. step_within, post_transform, vertical_note and void_note moved to raster/mod.rs; hgt, dted and read_scan_limited call them there. No behavior change.
- [x] 7. ELEVATION_EXTENSIONS and is_elevation_name in raster/mod.rs; used by elevation_ignores, read_path_placing, read_bytes, the web picker, the desktop picker and the folder browser; a test holds READ_EXTENSIONS to it.
- [ ] 8. One hgt check call.
- [ ] 2. DTED record counts.
- [ ] 3. Steps that don't divide.
- [ ] 4. HGT size message.
- [ ] 5. Ignored-option notice.
- [ ] 9. Reduced-reading efficiency.
