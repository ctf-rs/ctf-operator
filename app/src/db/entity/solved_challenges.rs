use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "solved_challenges")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub challenge_name: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub ctf_name: String,
    #[sea_orm(default_expr = "Expr::current_timestamp()")]
    pub solved_at: DateTimeWithTimeZone,

    // Relations
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::users::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
