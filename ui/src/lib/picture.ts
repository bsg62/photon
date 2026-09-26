/** Whether a re-read of the photo on screen shows a different *picture*, as opposed to
 *  different facts about the same one. A different picture reloads the viewer, which blanks
 *  the photo, resets zoom and pan and closes the crop tool - so this has to be exact. */

import type { ViewerItem } from './api';

type Picture = Pick<ViewerItem, 'thumbKey' | 'width' | 'height' | 'orientation' | 'thumbState' | 'kind'>;

/** The key covers the file and the edit; the dimensions and orientation cover a re-read of
 *  the same bytes by a newer reader.
 *
 *  `thumbState` counts only across `failed`: that is a photo that could not be shown and now
 *  can, or the reverse. `pending` → `ready` is the thumbnail being *made*, which changes
 *  nothing on screen, and comparing the states plainly made it count. Every edit sends the
 *  row back to `pending`, the viewer re-reads it before the worker is done, and the *next*
 *  library change of any kind - a star, a scan finishing - then saw `ready` against the
 *  `pending` it was holding and reloaded the photo, mid-crop if need be.
 *
 *  For a video, `thumbState` is dropped from the comparison entirely rather than narrowed to
 *  `failed`: it describes the *poster frame*, not what is on screen, which is the video
 *  itself once it exists at all. A poster arriving, or one that failed to render and is
 *  retried, must not pause and blank a video the user is watching - the viewer already reads
 *  playability from `videoState` reactively (`Viewer.svelte`'s `videoIssue`), not from this. */
export function pictureChanged(old: Picture, fresh: Picture): boolean {
  return (
    old.thumbKey !== fresh.thumbKey ||
    old.width !== fresh.width ||
    old.height !== fresh.height ||
    old.orientation !== fresh.orientation ||
    (old.kind !== 'video' && (old.thumbState === 'failed') !== (fresh.thumbState === 'failed'))
  );
}
