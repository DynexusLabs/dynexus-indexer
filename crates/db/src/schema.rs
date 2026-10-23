// Mirrors migrations/; regenerate with `diesel print-schema` after changing them.

diesel::table! {
    dynexus_events (transaction_version, event_index) {
        transaction_version -> Int8,
        event_index -> Int8,
        module -> Text,
        name -> Text,
        account -> Nullable<Text>,
        data -> Jsonb,
        ts -> Timestamptz,
    }
}

diesel::table! {
    tokens (address) {
        address -> Text,
        creator -> Text,
        name -> Nullable<Text>,
        symbol -> Nullable<Text>,
        decimals -> Nullable<Int2>,
        icon_uri -> Nullable<Text>,
        project_uri -> Nullable<Text>,
        burned -> Numeric,
        created_at -> Timestamptz,
        created_version -> Int8,
    }
}

diesel::table! {
    pools (address) {
        address -> Text,
        token_x -> Text,
        token_y -> Text,
        creator -> Text,
        reserve_x -> Numeric,
        reserve_y -> Numeric,
        lp_supply -> Numeric,
        fee_bps -> Int4,
        platform_fee_bps -> Int4,
        created_at -> Timestamptz,
        created_version -> Int8,
        last_version -> Int8,
    }
}

diesel::table! {
    swaps (transaction_version, event_index) {
        transaction_version -> Int8,
        event_index -> Int8,
        pool -> Text,
        trader -> Text,
        token_in -> Text,
        token_out -> Text,
        amount_in -> Numeric,
        amount_out -> Numeric,
        swap_fee -> Numeric,
        platform_fee -> Numeric,
        reserve_x -> Numeric,
        reserve_y -> Numeric,
        ts -> Timestamptz,
    }
}

diesel::table! {
    liquidity_events (transaction_version, event_index) {
        transaction_version -> Int8,
        event_index -> Int8,
        pool -> Text,
        provider -> Text,
        kind -> Text,
        amount_x -> Numeric,
        amount_y -> Numeric,
        lp_amount -> Numeric,
        reserve_x -> Numeric,
        reserve_y -> Numeric,
        ts -> Timestamptz,
    }
}

diesel::table! {
    ve_locks (user_address) {
        user_address -> Text,
        amount -> Numeric,
        unlock_time -> Int8,
        updated_version -> Int8,
    }
}

diesel::table! {
    votes (user_address, epoch, pool) {
        user_address -> Text,
        epoch -> Int8,
        pool -> Text,
        vote_weight -> Numeric,
        transaction_version -> Int8,
    }
}

diesel::table! {
    epochs (epoch) {
        epoch -> Int8,
        emission -> Numeric,
        total_votes -> Numeric,
        distributed_version -> Int8,
    }
}

diesel::table! {
    gauges (pool) {
        pool -> Text,
        lp_token -> Text,
        total_staked -> Numeric,
        reward_rate -> Numeric,
        period_finish -> Int8,
        created_version -> Int8,
    }
}

diesel::table! {
    gauge_stakes (pool, user_address) {
        pool -> Text,
        user_address -> Text,
        staked -> Numeric,
    }
}

diesel::table! {
    bribes (pool, epoch, token) {
        pool -> Text,
        epoch -> Int8,
        token -> Text,
        amount -> Numeric,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    dynexus_events,
    tokens,
    pools,
    swaps,
    liquidity_events,
    ve_locks,
    votes,
    epochs,
    gauges,
    gauge_stakes,
    bribes,
);
