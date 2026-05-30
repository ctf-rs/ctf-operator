use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "teams")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub team_id: String,
    pub team_name: String,
    #[sea_orm(default_expr = "Expr::current_timestamp()")]
    pub created_at: DateTimeWithTimeZone,
}

impl ActiveModelBehavior for ActiveModel {}
