# Review fixes for srtm-dted-2076 (temporary; deleted in the last commit)

- [x] 1. DTED checksums: read as stored, counted in a notice
- [x] 2. DTED two's complement below -16000
- [x] 6. DTED level from the DSI
- [ ] 3. HGT 7201 sizes, cut-short hint
- [ ] 4. Stepped post_transform: kept post at its cell's center
- [ ] 5. Multi-byte char after the tile code
- [ ] 7. crs/layer ignored notice for hgt/dted
- [ ] 8. Python embed list
- [ ] 9. read_scan_limited uses step_within if identical

Next: hgt.rs fixes 3, 4, 5.
