use std::ops::{Deref, DerefMut};

pub struct SelfUpdating<T>(Option<T>);

impl<T> SelfUpdating<T> {
    pub fn of(initial: T) -> Self {
        Self(Some(initial))
    }

    pub fn update<F: FnOnce(T) -> T>(&mut self, op: F) {
        self.0 = Some(op(self.0.take().unwrap()))
    }


}

impl<T> From<T> for SelfUpdating<T> {
    fn from(r: T) -> Self {
        SelfUpdating::of(r)
    }
}
impl<T> Deref for SelfUpdating<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}

impl<T> DerefMut for SelfUpdating<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().unwrap()
    }
}

mod tests {
    #[test]
    fn self_updating_init() {
        let self_updating=SelfUpdating::of(String::from("t"));
        match self_updating.0 {
            Some(v) => assert_eq!(v,"t"),
            _ => panic!("invalid state")
        }
    }

    #[test]
    fn self_updating_unwrap() {
        let self_updating=SelfUpdating::of(String::from("t"));
        assert_eq!(self_updating.unwrap(),"t")
    }

    #[test]
    fn self_updating_deref() {
        let self_updating=SelfUpdating::of(String::from("t"));
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

    #[test]
    #[should_panic]
    fn self_updating_consume() {
        let mut self_updating=SelfUpdating::of(String::from("test"));
        self_updating.consume(|s| panic!(s));
    }

    #[test]
    fn self_updating_perform_returning_update() {
        let mut self_updating=SelfUpdating::of(String::from("test"));
        let res=self_updating.returning_update(|s| (s.repeat(2),0.3));
        assert_eq!(self_updating.unwrap(),"testtest");
        assert_eq!(res,0.3);
    }

}