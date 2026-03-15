use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique)]
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    #[sea_orm(default_expr = "Expr::current_timestamp()")]
    pub created_at: DateTimeWithTimeZone,

    // Relations
    #[sea_orm(has_many)]
    pub sessions: HasMany<super::sessions::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
