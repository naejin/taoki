// Expected: exit 0
// Expected: contains=mod:
// Expected: contains=outer

pub mod outer {
    pub mod middle {
        pub struct Inner {
            pub value: i32,
        }

        impl Inner {
            pub fn new(value: i32) -> Self {
                Self { value }
            }
        }
    }
}
