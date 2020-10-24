use std::ops::{Deref, DerefMut};

pub struct SelfUpdating<T>(Option<T>);

impl<T> SelfUpdating<T> {
    pub fn of(initial: T) -> Self {
        Self(Some(initial))
    }

    pub fn update<F: Fn(T) -> T>(&mut self, op: F) {
        self.0 = Some(op(self.0.take().unwrap()))
    }

    pub fn unwrap(mut self) -> T {
        self.0?
    }
}

impl<T> Deref for SelfUpdating<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()?
    }
}

impl<T> DerefMut for SelfUpdating<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut()?
    }
}

mod tests {
    use crate::utils::SelfUpdating;

    #[test]
    fn self_updating_init() {
        let mut self_updating=SelfUpdating::of(String::from("t"));
        match self_updating.0 {
            Some(v) => assert_eq!(v,"t"),
            _ => panic!("invalid state")
        }
    }

    #[test]
    fn self_updating_unwrap() {
        let mut self_updating=SelfUpdating::of(String::from("t"));
        assert_eq!(self_updating.unwrap(),"t")
    }

    #[test]
    fn self_updating_deref() {
        let mut self_updating=SelfUpdating::of(String::from("t"));
        assert_eq!(self_updating.len(),1);
    }

    #[test]
    fn self_updating_deref_mut() {
        let mut self_updating=SelfUpdating::of(String::from("test"));
        assert_eq!(self_updating.remove(0),'t');
    }
    #[test]
    fn self_updating_perform_update() {
        let mut self_updating=SelfUpdating::of(String::from("test"));
        self_updating.update(|s| s.repeat(2));
        assert_eq!(self_updating.unwrap(),"testtest");
    }

}