//! Which workspace areas are visible. The center (tabs) and the terminal
//! dock share the right side; at least one of them always stays visible.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Sidebar,
    Center,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub sidebar: bool,
    pub center: bool,
    pub terminal: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar: true,
            center: true,
            terminal: true,
        }
    }
}

impl Layout {
    pub fn toggle(&mut self, area: Area) {
        match area {
            Area::Sidebar => self.sidebar = !self.sidebar,
            Area::Center => {
                self.center = !self.center;
                if !self.center {
                    self.terminal = true;
                }
            }
            Area::Terminal => {
                self.terminal = !self.terminal;
                if !self.terminal {
                    self.center = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Area, Layout};

    #[test]
    fn everything_starts_visible() {
        let l = Layout::default();
        assert!(l.sidebar && l.center && l.terminal);
    }

    #[test]
    fn toggling_twice_restores() {
        let mut l = Layout::default();
        for area in [Area::Sidebar, Area::Center, Area::Terminal] {
            l.toggle(area);
            l.toggle(area);
            assert_eq!(l, Layout::default());
        }
    }

    #[test]
    fn hiding_the_last_right_side_area_shows_the_other() {
        let mut l = Layout::default();
        l.toggle(Area::Terminal);
        assert!(!l.terminal && l.center);
        l.toggle(Area::Center);
        assert!(!l.center && l.terminal, "terminal comes back instead");
        l.toggle(Area::Terminal);
        assert!(!l.terminal && l.center);
    }

    #[test]
    fn sidebar_is_independent() {
        let mut l = Layout::default();
        l.toggle(Area::Sidebar);
        assert!(!l.sidebar && l.center && l.terminal);
    }
}
