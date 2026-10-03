/** What the main area shows: the grid, or the People page. The People page is not a grid
 *  view - it has no rows - so it is not `GridView` and does not travel the view chain; the
 *  grid keeps whatever view it had, and stays mounted, hidden, beneath the page (App.svelte
 *  says why). */
export type MainPage = 'grid' | 'people';

class MainPageState {
  current = $state<MainPage>('grid');
  showGrid() {
    this.current = 'grid';
  }
  showPeople() {
    this.current = 'people';
  }
}

export const mainPage = new MainPageState();
