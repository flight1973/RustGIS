# Review round 2 notes (removed in the last commit)

- [x] 6. step_within, post_transform, vertical_note and void_note moved to raster/mod.rs; hgt, dted and read_scan_limited call them there. No behavior change.
- [x] 7. ELEVATION_EXTENSIONS and is_elevation_name in raster/mod.rs; used by elevation_ignores, read_path_placing, read_bytes, the web picker, the desktop picker and the folder browser; a test holds READ_EXTENSIONS to it.
- [x] 8. read_path_placing calls hgt::check once and passes its result to hgt::read_checked, which holds the bytes to the size checked; hgt::read keeps name-and-bytes callers.
- [x] 2. Only a bad sentinel makes a DTED column damaged; records whose counts disagree with their place are read by place and counted in one notice; refused only when no record has the sentinel. Test: records_numbered_from_1_are_read_by_place.
- [x] 3. Read at step k, overview is (width·k, height·k) in dted and hgt, so overview / width == step == file_pixels and the warp scale. Extent unchanged (cells centered on kept posts). Test: a_step_that_doesnt_divide_the_posts_keeps_the_scale_exact (9 × 13 at step 2).
- [x] 4. A wrong-size .hgt gives the nearest tile sizes (n bytes more than one, m fewer than the next); "may be cut short" only below 1201 × 1201. Tests: 1201² + 2, 3601² + 2, 7201² ± 2, 0 bytes.
- [x] 5. elevation_ignores no longer says DTED is always WGS 84: the "crs" notice says the files give their own coordinates, read as WGS 84 (a DTED file naming another datum says so); the "layer" notice says they hold one band. Engine tests pin all three texts.
- [x] 9. DTED: columns skipped at a step are only counted (voids and two's complement, via twos_complement), not decoded or pushed. HGT: one pass counts voids and keeps every step-th sample. Test: counts_at_a_step_are_the_whole_files.
