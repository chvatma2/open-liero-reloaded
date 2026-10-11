# OpenLieroX 0.58 rc5 skins

The worm skins that OpenLieroX 0.58 rc5's Windows release
(`OpenLieroX_0.58_rc5.win32`, its `OpenLieroX/skins/` folder) ships and the
OpenLieroX repository at the pinned commit (`content/olx-manifest.ron`)
lacks: 309 PNG files, copied unchanged. The same names in the repository
are not repeated here.

`cargo xtask fetch-olx-content` imports them with the repository's content
(`extra_skins` in the manifest), each pack's origin naming this release.
Two of them (`Teletubbie.png`, `capworm.png`) are Windows bitmaps under a
`.png` name, as OLX shipped them; the importer reads them as OLX did.

Delete a file to stop bundling that skin.
