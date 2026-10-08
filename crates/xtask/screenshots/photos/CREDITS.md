# Photos for the screenshots

Served in place of the generated gradients by `cargo run -p xtask -- screenshots --photos crates/xtask/screenshots/photos`,
which is how the project website's screenshots are made. Item id *n* shows file *n*, wrapping round
after the last. `03.jpg` is the photo the viewer shots open, cropped so the mock's face rectangle
(`mock.js`, Anna) falls on the person in it; the rest are shrunk to thumbnail size.
`thumbs/03.jpg` is that photo at thumbnail size (`magick 03.jpg -resize 480x -strip -quality 85`):
a grid tile and a face crop are served the copy in `thumbs/` when a photo has one, because a
1600px photo in a tile was sometimes not decoded when the screenshot was taken.

Every photo is from Wikimedia Commons and was marked CC0 or public domain there when it was
taken (2026-09-29). None of them needs attribution; it is kept here so each can be checked.

| File | Source | Author | Licence |
|---|---|---|---|
| 01.jpg | [20260712 145215 HDR.jpg](https://commons.wikimedia.org/wiki/File:20260712_145215_HDR.jpg) | Harald Hetzner | CC0 |
| 02.jpg | [20250815 105328 HDR.jpg](https://commons.wikimedia.org/wiki/File:20250815_105328_HDR.jpg) | Harald Hetzner | CC0 |
| 03.jpg | [Woman overlooking Lisbon sunset (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Woman_overlooking_Lisbon_sunset_(Unsplash).jpg) | John Sting joaoferrao | CC0 |
| 04.jpg | [20260712 151124 HDR.jpg](https://commons.wikimedia.org/wiki/File:20260712_151124_HDR.jpg) | Harald Hetzner | CC0 |
| 05.jpg | [Adventurous Mountain Hikes (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Adventurous_Mountain_Hikes_(Unsplash).jpg) | Galen Crout galen_crout | CC0 |
| 06.jpg | [Capra ibex in Hribarice 04.jpg](https://commons.wikimedia.org/wiki/File:Capra_ibex_in_Hribarice_04.jpg) | Janezdrilc | CC0 |
| 07.jpg | [Comastoma nanum (Zwerg-Haarschlund) IMG 2745.JPG](https://commons.wikimedia.org/wiki/File:Comastoma_nanum_(Zwerg-Haarschlund)_IMG_2745.JPG) | HermannSchachner | CC0 |
| 08.jpg | [Jade Lake view from the shore.jpg](https://commons.wikimedia.org/wiki/File:Jade_Lake_view_from_the_shore.jpg) | SpikyLlama | CC0 |
| 09.jpg | [Matterhorn sunset 2016 (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Matterhorn_sunset_2016_(Unsplash).jpg) | Sam Ferrara samferrara | CC0 |
| 10.jpg | [20250906 111730 HDR.jpg](https://commons.wikimedia.org/wiki/File:20250906_111730_HDR.jpg) | Harald Hetzner | CC0 |
| 11.jpg | [Hiking at Lulin Front Mountain - 2026 (IMGP1343).jpg](https://commons.wikimedia.org/wiki/File:Hiking_at_Lulin_Front_Mountain_-_2026_(IMGP1343).jpg) | 小 葵 | CC0 |
| 12.jpg | [Groundhog, 2023-04-25 - 2.jpg](https://commons.wikimedia.org/wiki/File:Groundhog,_2023-04-25_-_2.jpg) | auburn | CC0 |
| 13.jpg | [Autumn Lake Pukaki NZ.jpg](https://commons.wikimedia.org/wiki/File:Autumn_Lake_Pukaki_NZ.jpg) | Bernard Spragg. NZ | CC0 |
| 14.jpg | [Hallstatt Austria Bergsee Lake Alpine Summer.jpg](https://commons.wikimedia.org/wiki/File:Hallstatt_Austria_Bergsee_Lake_Alpine_Summer.jpg) | Julius_Silver | CC0 |
| 15.jpg | [Hohe Tauern1.jpg](https://commons.wikimedia.org/wiki/File:Hohe_Tauern1.jpg) | Ted Moravec | CC0 |
| 16.jpg | [20260712 145311 HDR.jpg](https://commons.wikimedia.org/wiki/File:20260712_145311_HDR.jpg) | Harald Hetzner | CC0 |
| 17.jpg | [Female Cabra Ibex in Alpenzoo Innsbruck.jpg](https://commons.wikimedia.org/wiki/File:Female_Cabra_Ibex_in_Alpenzoo_Innsbruck.jpg) | Wilfredor | CC0 |
| 18.jpg | [Chill (205080241).jpeg](https://commons.wikimedia.org/wiki/File:Chill_(205080241).jpeg) | Ioan Sendroiu | CC0 |
| 19.jpg | [MindelheimerHütteAlexanderHauk.jpg](https://commons.wikimedia.org/wiki/File:MindelheimerH%C3%BCtteAlexanderHauk.jpg) | Alexander Hauk / alexander-hauk.de | CC0 |
| 20.jpg | [Mountain hiking sunset follow @kalenemsley on ig (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Mountain_hiking_sunset_follow_@kalenemsley_on_ig_(Unsplash).jpg) | Kalen Emsley kalenemsley | CC0 |
| 21.jpg | [Marmot near Mine de l'Herpie, Huez, 2026.jpg](https://commons.wikimedia.org/wiki/File:Marmot_near_Mine_de_l%27Herpie,_Huez,_2026.jpg) | DimiTalen | CC0 |
| 22.jpg | [Opening in the Spring (Unsplash).jpg](https://commons.wikimedia.org/wiki/File:Opening_in_the_Spring_(Unsplash).jpg) | Daniel Chen d_che | CC0 |
| 23.jpg | [Wetterspitzen (Stubaier Alpen).jpg](https://commons.wikimedia.org/wiki/File:Wetterspitzen_(Stubaier_Alpen).jpg) | Jörg Braukmann | CC0 |
