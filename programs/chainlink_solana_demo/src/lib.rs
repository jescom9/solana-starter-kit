use anchor_lang::prelude::*;

use chainlink_solana as chainlink;

declare_id!("41Np7rprA1XXuJ7k83PMh6e5adpyFkdJ2NPh1sGd72A9");

#[program]
pub mod chainlink_solana_demo {
    use super::*;

    pub fn initialize_asset_registry(ctx: Context<InitializeAssetRegistry>) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;
        registry.authority = ctx.accounts.authority.key();
        registry.assets = Vec::new();
        registry.risk_params = Vec::new();

        msg!(
            "Asset Registry initialized with authority: {}",
            registry.authority
        );
        Ok(())
    }

    pub fn add_asset(
        ctx: Context<ManageAssetRegistry>,
        id: u8,
        price: u64,
        decimals: u8,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;

        // Check if ID already exists
        if registry.assets.iter().any(|a| a.id == id) {
            return Err(ErrorCode::AssetAlreadyExists.into());
        }

        registry.assets.push(AssetInfo {
            id,
            price,
            decimals,
        });

        msg!(
            "Added asset: id={}, price={}, decimals={}",
            id,
            price,
            decimals
        );
        Ok(())
    }

    pub fn update_asset_price(
        ctx: Context<ManageAssetRegistry>,
        id: u8,
        new_price: u64,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;

        let asset = registry
            .assets
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(ErrorCode::AssetNotFound)?;

        asset.price = new_price;

        msg!("Updated asset {} price to {}", id, new_price);
        Ok(())
    }

    pub fn add_risk_param(
        ctx: Context<ManageAssetRegistry>,
        asset_id_a: u8,
        asset_id_b: u8,
        risk_level: u8,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;

        // Verify both assets exist
        if !registry.assets.iter().any(|a| a.id == asset_id_a) {
            return Err(ErrorCode::AssetNotFound.into());
        }
        if !registry.assets.iter().any(|a| a.id == asset_id_b) {
            return Err(ErrorCode::AssetNotFound.into());
        }

        // Check if pair already exists
        if registry.risk_params.iter().any(|p| {
            (p.asset_id_a == asset_id_a && p.asset_id_b == asset_id_b)
                || (p.asset_id_a == asset_id_b && p.asset_id_b == asset_id_a)
        }) {
            return Err(ErrorCode::RiskParamAlreadyExists.into());
        }

        registry.risk_params.push(PairRiskParam {
            asset_id_a,
            asset_id_b,
            risk_level,
        });

        msg!(
            "Added risk param: assets {}-{}, level={}",
            asset_id_a,
            asset_id_b,
            risk_level
        );
        Ok(())
    }

    // ========== OBLIGATION INSTRUCTIONS ==========

    pub fn init_obligation(ctx: Context<InitObligation>) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;
        obligation.owner = ctx.accounts.owner.key();
        obligation.deposits = Vec::new();
        obligation.borrows = Vec::new();

        msg!("Obligation initialized for owner: {}", obligation.owner);
        Ok(())
    }

    pub fn add_deposit(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;
        let registry = &ctx.accounts.asset_registry;

        // Verify asset exists
        if !registry.assets.iter().any(|a| a.id == asset_id) {
            return Err(ErrorCode::AssetNotFound.into());
        }

        // Add or update deposit
        if let Some(position) = obligation
            .deposits
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
        {
            position.amount = position
                .amount
                .checked_add(amount)
                .ok_or(ErrorCode::MathOverflow)?;
        } else {
            obligation.deposits.push(Position { asset_id, amount });
        }

        msg!("Added deposit: asset_id={}, amount={}", asset_id, amount);

        // Perform health check
        perform_health_check(&ctx.accounts.obligation, &ctx.accounts.asset_registry)?;

        Ok(())
    }

    pub fn add_borrow(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;
        let registry = &ctx.accounts.asset_registry;

        // Verify asset exists
        if !registry.assets.iter().any(|a| a.id == asset_id) {
            return Err(ErrorCode::AssetNotFound.into());
        }

        msg!("Adding borrow: asset_id={}, amount={}", asset_id, amount);
        msg!(
            "Current deposits: {}, borrows: {}",
            obligation.deposits.len(),
            obligation.borrows.len()
        );

        // Add or update borrow
        if let Some(position) = obligation
            .borrows
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
        {
            position.amount = position
                .amount
                .checked_add(amount)
                .ok_or(ErrorCode::MathOverflow)?;
        } else {
            obligation.borrows.push(Position { asset_id, amount });
        }

        // Perform health check
        perform_health_check(&ctx.accounts.obligation, &ctx.accounts.asset_registry)?;

        Ok(())
    }

    pub fn remove_deposit(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;

        msg!("Removing deposit: asset_id={}, amount={}", asset_id, amount);
        msg!(
            "Current deposits: {}, borrows: {}",
            obligation.deposits.len(),
            obligation.borrows.len()
        );

        if amount == 0 {
            return Ok(());
        }

        let position = obligation
            .deposits
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
            .ok_or(ErrorCode::DepositNotFound)?;

        if position.amount < amount {
            return Err(ErrorCode::InsufficientDeposit.into());
        }

        position.amount = position.amount.checked_sub(amount).unwrap();

        // Remove if zero
        if position.amount == 0 {
            obligation.deposits.retain(|p| p.asset_id != asset_id);
        }

        // Perform health check
        perform_health_check(&ctx.accounts.obligation, &ctx.accounts.asset_registry)?;

        Ok(())
    }

    pub fn remove_borrow(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;

        msg!("Removing borrow: asset_id={}, amount={}", asset_id, amount);

        if amount == 0 {
            return Ok(());
        }

        let position = obligation
            .borrows
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
            .ok_or(ErrorCode::BorrowNotFound)?;

        if position.amount < amount {
            return Err(ErrorCode::InsufficientBorrow.into());
        }

        position.amount = position.amount.checked_sub(amount).unwrap();

        // Remove if zero
        if position.amount == 0 {
            obligation.borrows.retain(|p| p.asset_id != asset_id);
        }

        // Perform health check
        perform_health_check(&ctx.accounts.obligation, &ctx.accounts.asset_registry)?;

        Ok(())
    }

    // ========== DEBUG INSTRUCTION ==========

    pub fn debug_read_all_data(ctx: Context<DebugReadData>) -> Result<()> {
        let registry = &ctx.accounts.asset_registry;
        let obligation = &ctx.accounts.obligation;

        msg!("=== ASSET REGISTRY DATA ===");
        msg!("Authority: {}", registry.authority);
        msg!("Total assets: {}", registry.assets.len());

        for asset in &registry.assets {
            msg!(
                "Asset: id={}, price={}, decimals={}",
                asset.id,
                asset.price,
                asset.decimals
            );
        }

        msg!("Total risk params: {}", registry.risk_params.len());
        for param in &registry.risk_params {
            msg!(
                "Risk param: {}-{}, level={}",
                param.asset_id_a,
                param.asset_id_b,
                param.risk_level
            );
        }

        msg!("=== OBLIGATION DATA ===");
        msg!("Owner: {}", obligation.owner);
        msg!("Deposits: {}", obligation.deposits.len());
        for deposit in &obligation.deposits {
            msg!(
                "  Deposit: asset_id={}, amount={}",
                deposit.asset_id,
                deposit.amount
            );
        }

        msg!("Borrows: {}", obligation.borrows.len());
        for borrow in &obligation.borrows {
            msg!(
                "  Borrow: asset_id={}, amount={}",
                borrow.asset_id,
                borrow.amount
            );
        }

        Ok(())
    }

    use chainlink_solana::Round;

    use super::*;
    pub fn execute(ctx: Context<Execute>) -> Result<()> {
        let round: Round = chainlink::latest_round_data(
            ctx.accounts.chainlink_program.to_account_info(),
            ctx.accounts.chainlink_feed.to_account_info(),
        )?;

        let description: String = chainlink::description(
            ctx.accounts.chainlink_program.to_account_info(),
            ctx.accounts.chainlink_feed.to_account_info(),
        )?;

        let decimals: u8 = chainlink::decimals(
            ctx.accounts.chainlink_program.to_account_info(),
            ctx.accounts.chainlink_feed.to_account_info(),
        )?;

        let decimal: &mut Account<Decimal> = &mut ctx.accounts.decimal;
        decimal.value = round.answer;
        decimal.decimals = u32::from(decimals);

        let decimal_print: Decimal = Decimal::new(round.answer, u32::from(decimals));
        msg!("{} price is {}", description, decimal_print);
        Ok(())
    }
}

// ========== HEALTH CHECK FUNCTION ==========

fn perform_health_check(obligation: &Obligation, registry: &AssetRegistry) -> Result<()> {
    msg!("=== HEALTH CHECK START ===");
    msg!(
        "Deposits: {}, Borrows: {}",
        obligation.deposits.len(),
        obligation.borrows.len()
    );

    // If no borrows, obligation is healthy by default
    if obligation.borrows.is_empty() {
        msg!("Health: OK (no borrows)");
        return Ok(());
    }

    // Calculate total deposit and borrow values
    let mut deposit_values: Vec<(u8, u64)> = Vec::new(); // (asset_id, value)
    let mut borrow_values: Vec<(u8, u64)> = Vec::new(); // (asset_id, value)
    let mut total_deposit_value = 0u64;
    let mut total_borrow_value = 0u64;

    // Calculate deposit values
    for deposit in &obligation.deposits {
        let asset = registry
            .assets
            .iter()
            .find(|a| a.id == deposit.asset_id)
            .ok_or(ErrorCode::AssetNotFound)?;

        let value = deposit.amount.saturating_mul(asset.price);
        deposit_values.push((deposit.asset_id, value));
        total_deposit_value = total_deposit_value.saturating_add(value);

        msg!(
            "Deposit: id={}, amount={}, price={}, value={}",
            deposit.asset_id,
            deposit.amount,
            asset.price,
            value
        );
    }

    // Calculate borrow values
    for borrow in &obligation.borrows {
        let asset = registry
            .assets
            .iter()
            .find(|a| a.id == borrow.asset_id)
            .ok_or(ErrorCode::AssetNotFound)?;

        let value = borrow.amount.saturating_mul(asset.price);
        borrow_values.push((borrow.asset_id, value));
        total_borrow_value = total_borrow_value.saturating_add(value);

        msg!(
            "Borrow: id={}, amount={}, price={}, value={}",
            borrow.asset_id,
            borrow.amount,
            asset.price,
            value
        );
    }

    msg!("Total deposit value: {}", total_deposit_value);
    msg!("Total borrow value: {}", total_borrow_value);

    // Complex health score calculation
    // Health = Sum(deposit_i * Sum(borrow_share_j * risk_factor_ij)) / total_borrow
    let mut weighted_health_score = 0u64;

    // For each deposit
    for (deposit_id, deposit_value) in &deposit_values {
        msg!("Processing deposit {}: value={}", deposit_id, deposit_value);
        let mut deposit_risk_sum = 0u64;

        // For each borrow
        for (borrow_id, borrow_value) in &borrow_values {
            // Find risk parameter for this deposit-borrow pair
            let risk_param = registry.risk_params.iter().find(|p| {
                (p.asset_id_a == *deposit_id && p.asset_id_b == *borrow_id)
                    || (p.asset_id_a == *borrow_id && p.asset_id_b == *deposit_id)
            });

            let risk_level = if let Some(param) = risk_param {
                param.risk_level as u64
            } else {
                // Default risk level if pair not found
                msg!(
                    "  Warning: No risk param for pair {}-{}, using default 50",
                    deposit_id,
                    borrow_id
                );
                50
            };

            // Calculate borrow share (scaled by 100 for precision)
            let borrow_share = borrow_value
                .saturating_mul(100)
                .checked_div(total_borrow_value)
                .unwrap_or(0);

            // Add to deposit risk sum: borrow_share * risk_level
            let risk_contribution = borrow_share.saturating_mul(risk_level);
            deposit_risk_sum = deposit_risk_sum.saturating_add(risk_contribution);

            msg!(
                "  Pair {}-{}: borrow_value={}, share={}%, risk={}, contribution={}",
                deposit_id,
                borrow_id,
                borrow_value,
                borrow_share,
                risk_level,
                risk_contribution
            );
        }

        // Multiply deposit value by its weighted risk (divide by 100 to adjust for scaling)
        let weighted_deposit_value = deposit_value
            .saturating_mul(deposit_risk_sum)
            .checked_div(100)
            .unwrap_or(0);

        weighted_health_score = weighted_health_score.saturating_add(weighted_deposit_value);

        msg!(
            "  Deposit {} total: risk_sum={}, weighted_value={}",
            deposit_id,
            deposit_risk_sum,
            weighted_deposit_value
        );
    }

    // Final health score calculation
    // We have: weighted_health_score (already divided by 100 once)
    // Need to divide by total_borrow_value
    // But we want to keep decimal precision, so multiply by 1000 first
    let final_health_score_x1000 = weighted_health_score
        .saturating_mul(1000)
        .checked_div(total_borrow_value)
        .unwrap_or(0)
        .checked_div(100) // Divide by 100 for the risk scaling
        .unwrap_or(0);

    msg!("=== FINAL CALCULATION ===");
    msg!("Weighted health score sum: {}", weighted_health_score);
    msg!("Total borrow value: {}", total_borrow_value);
    msg!("Health score x1000: {}", final_health_score_x1000);
    msg!(
        "Health score: {}.{}",
        final_health_score_x1000 / 1000,
        final_health_score_x1000 % 1000
    );

    // Check if healthy (health score should be >= 1000 for 1.0 or 100% collateralization)
    if final_health_score_x1000 < 1000 {
        msg!(
            "⚠️ WARNING: Health score {}.{} is below 1.0 - Position at risk!",
            final_health_score_x1000 / 1000,
            final_health_score_x1000 % 1000
        );
        return Err(ErrorCode::Unhealthy.into());
    } else {
        msg!(
            "✓ Health check PASSED - Score: {}.{}",
            final_health_score_x1000 / 1000,
            final_health_score_x1000 % 1000
        );
    }

    msg!("=== HEALTH CHECK END ===");
    Ok(())
}

// ========== CONTEXTS ==========

#[derive(Accounts)]
pub struct InitializeAssetRegistry<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + AssetRegistry::INIT_SPACE,
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ManageAssetRegistry<'info> {
    #[account(
        mut,
        seeds = [b"asset_registry"],
        bump,
        has_one = authority
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct InitObligation<'info> {
    #[account(
        init,
        payer = owner,
        space = 8 + Obligation::INIT_SPACE,
        seeds = [b"obligation", owner.key().as_ref()],
        bump
    )]
    pub obligation: Account<'info, Obligation>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ModifyObligation<'info> {
    #[account(
        mut,
        seeds = [b"obligation", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub obligation: Account<'info, Obligation>,
    #[account(
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct DebugReadData<'info> {
    #[account(
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    #[account(
        seeds = [b"obligation", obligation.owner.as_ref()],
        bump
    )]
    pub obligation: Account<'info, Obligation>,
}

// ========== ERROR CODES ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Asset already exists with this ID")]
    AssetAlreadyExists,
    #[msg("Asset not found in registry")]
    AssetNotFound,
    #[msg("Risk parameter already exists for this pair")]
    RiskParamAlreadyExists,
    #[msg("Deposit not found in obligation")]
    DepositNotFound,
    #[msg("Borrow not found in obligation")]
    BorrowNotFound,
    #[msg("Insufficient deposit amount")]
    InsufficientDeposit,
    #[msg("Insufficient borrow amount")]
    InsufficientBorrow,
    #[msg("Obligation health score is below minimum threshold")]
    Unhealthy,
    #[msg("Math operation overflowed")]
    MathOverflow,
}

// ========== DATA STRUCTURES ==========

#[account]
#[derive(InitSpace)]
pub struct AssetRegistry {
    pub authority: Pubkey,
    #[max_len(20)]
    pub assets: Vec<AssetInfo>,
    #[max_len(50)]
    pub risk_params: Vec<PairRiskParam>,
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq, Eq, InitSpace)]
pub struct AssetInfo {
    pub id: u8,
    pub price: u64,
    pub decimals: u8,
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq, Eq, InitSpace)]
pub struct PairRiskParam {
    pub asset_id_a: u8,
    pub asset_id_b: u8,
    pub risk_level: u8,
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq, Eq, InitSpace)]
pub struct Position {
    pub asset_id: u8,
    pub amount: u64,
}

#[account]
#[derive(InitSpace)]
pub struct Obligation {
    pub owner: Pubkey,
    #[max_len(11)]
    pub deposits: Vec<Position>,
    #[max_len(10)]
    pub borrows: Vec<Position>,
}

#[derive(Accounts)]
pub struct Execute<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        init,
        payer = user,
        space = 100,
    )]
    pub decimal: Account<'info, Decimal>,

    /// CHECK: We're reading data from this specified chainlink feed
    pub chainlink_feed: AccountInfo<'info>,
    /// CHECK: This is the Chainlink program library on Devnet
    pub chainlink_program: AccountInfo<'info>,
    /// CHECK: This is the devnet system program
    pub system_program: Program<'info, System>,
}

#[account]
pub struct Decimal {
    pub value: i128,
    pub decimals: u32,
}

impl Decimal {
    pub fn new(value: i128, decimals: u32) -> Self {
        Decimal { value, decimals }
    }
}

impl std::fmt::Display for Decimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut scaled_val = self.value.to_string();
        if scaled_val.len() <= self.decimals as usize {
            scaled_val.insert_str(
                0,
                &vec!["0"; self.decimals as usize - scaled_val.len()].join(""),
            );
            scaled_val.insert_str(0, "0.")
        } else {
            scaled_val.insert(scaled_val.len() - self.decimals as usize, '.');
        }
        f.write_str(&scaled_val)
    }
}
