# App icon

`icon.svg` is the source of every file here except the smallest sizes. `icon-small.svg` is the
same aperture drawn flat, with wider gaps and a larger point of light, because the shaded
blades of `icon.svg` blur into a grey disc at 16 and 24 pixels.

Regenerating, from the repository root:

```bash
rsvg-convert -w 1024 -h 1024 crates/photon-app/icons/icon.svg -o /tmp/icon-1024.png
npx tauri icon /tmp/icon-1024.png -o crates/photon-app/icons
```

then replace `32x32.png` and the 16, 24 and 32 pixel layers of `icon.ico` with renders of
`icon-small.svg`. Keep every `.ico` layer PNG-compressed: ImageMagick writes the 256 pixel
layer as an uncompressed bitmap, which makes the file eight times larger.
