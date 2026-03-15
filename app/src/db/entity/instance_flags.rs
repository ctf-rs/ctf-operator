use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "instance_flags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub instance_name: String,
    pub user_id: Option<i64>,
    pub challenge_name: String,
    pub ctf_name: String,
    pub flag_name: String,
    pub generated_flag: String,
    #[sea_orm(default_expr = "Expr::current_timestamp()")]
    pub created_at: DateTimeWithTimeZone,

    // Relations
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: Option<super::users::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
