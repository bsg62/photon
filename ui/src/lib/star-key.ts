/** What the star key and the menu need to know about a photo. */
export interface StarPhoto {
  id: number;
  starred: boolean;
}

/** The selected photo whose star decides which way the grid's star key goes: starred, the
 *  selection is unstarred; not, it is starred.
 *
 *  The lead - the photo an arrow key would move from - when it is in the selection. It is
 *  not always: a Ctrl+click that takes a photo out leaves the lead on it, and a rebuild
 *  can slide another photo under the lead's offset. Read regardless, the key took its
 *  direction from a photo with no ring on it, and a lone starred photo beside such a lead
 *  could not be unstarred at all. Then the first selected photo among `shown`, the ones on
 *  screen, stands in. `undefined` when no selected photo is at hand - a selection scrolled
 *  away from, its pages gone - which the caller reads as "star": starring twice is harmless. */
export function decidingPhoto<P extends StarPhoto>(
  lead: P | undefined,
  shown: Iterable<P | undefined>,
  isSelected: (id: number) => boolean,
): P | undefined {
  if (lead && isSelected(lead.id)) return lead;
  for (const photo of shown) {
    if (photo && isSelected(photo.id)) return photo;
  }
  return undefined;
}
