//! Stores a newtype in SQLite as the value it wraps, so queries bind and
//! read it as such.

/// Implements sqlx's `Type`, `Encode` and `Decode` for SQLite on the
/// generic newtype `$type<K>`, as its field `$field` of type `$inner`,
/// made back into one by `$new`.
macro_rules! sqlite_newtype {
    ($type:ident, $field:ident: $inner:ty, $new:path) => {
        impl<K> sqlx::Type<sqlx::Sqlite> for $type<K> {
            fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
                <$inner as sqlx::Type<sqlx::Sqlite>>::type_info()
            }

            fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
                <$inner as sqlx::Type<sqlx::Sqlite>>::compatible(ty)
            }
        }

        impl<'q, K> sqlx::Encode<'q, sqlx::Sqlite> for $type<K> {
            fn encode_by_ref(
                &self,
                args: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <$inner as sqlx::Encode<'q, sqlx::Sqlite>>::encode_by_ref(&self.$field, args)
            }
        }

        impl<'r, K> sqlx::Decode<'r, sqlx::Sqlite> for $type<K> {
            fn decode(
                value: sqlx::sqlite::SqliteValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                <$inner as sqlx::Decode<'r, sqlx::Sqlite>>::decode(value).map($new)
            }
        }
    };
}

pub(crate) use sqlite_newtype;
