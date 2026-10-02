# Face detection fixtures

| File | Source | Author | Licence |
|---|---|---|---|
| `portrait.jpg` | [Portrait of a man (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Portrait_of_a_man_(Unsplash).jpg), Commons' 960 px rendering | William Stitt | CC0 |

One face, looking at the camera. On 2026-10-02 the detector's large run (1280) put it at left
0.317, top 0.209, right 0.638, bottom 0.896 with a score of 0.936, and its small run (320) at
left 0.321, top 0.230, right 0.652, bottom 0.895 with 0.945. Both find it, so the one returned
is the stronger, the small run's; the tests hold it to the first set within 0.05.

The photo without a face is `crates/xtask/screenshots/photos/14.jpg` (credited there).
