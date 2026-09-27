//! Sub-`Par` splitting and free-variable filtering (port of `matcher/ParSpatialMatcherUtils.scala`).

use rchain_models::ast::{Expr, Par, Sort, Var};

use crate::errors::RholangError;
use crate::matcher::par_count::ParCount;

/// Cap on the number of items in a single subset-enumeration dimension. A connective pattern
/// matched against a datum with `n` top-level processes enumerates up to 2ⁿ (subset, complement)
/// splits; beyond this bound the enumeration is a denial-of-service, so it is rejected rather than
/// materialized (C-2, defense-in-depth).
///
/// **19 and not 20, because the value is derived rather than chosen** (AUDIT C124). This bound is
/// only a refusal if what it permits fits inside [`MAX_SPLIT_COMBINATIONS`] — and at 20 a single
/// dimension permits 2²⁰ = 1,048,576 pairs, which is *above* the product cap, so the product check
/// could only ever reject work that had already been materialized. That is the 16 GB: the guard was
/// real, the enumeration happened first, and the number that was supposed to prevent it permitted
/// it. [`subset_count`] now refuses from counts before anything is allocated, and
/// `a_single_dimension_cannot_outgrow_the_product_cap` keeps the two constants consistent.
pub const MAX_SUBSET_ITEMS: usize = 19;

/// Cap on the total number of split combinations produced by `sub_pars` (the 7-way Cartesian
/// product of the per-dimension subsets). Checked from *counts*, before anything is materialized —
/// see [`subset_count`] and AUDIT C124.
pub const MAX_SPLIT_COMBINATIONS: u64 = 1_000_000;

/// Remove free-variable/wildcard exprs from a `Par` (port of `noFrees`).
pub fn no_frees<S: Sort>(par: &Par<S>) -> Par<S> {
    Par {
        exprs: no_frees_exprs(&par.exprs),
        ..par.clone()
    }
}

/// Remove free-variable/wildcard exprs from a list (port of `noFrees(exprs)`).
pub fn no_frees_exprs(exprs: &[Expr]) -> Vec<Expr> {
    exprs
        .iter()
        .filter(|expr| match expr {
            Expr::EVar(v) => matches!(**v, Var::BoundVar(_) | Var::Empty),
            _ => true,
        })
        .cloned()
        .collect()
}

/// The number of `(subset, complement)` pairs `min_max_subsets` **would** produce for `len` items,
/// computed without producing them: `Σ_{k=lo}^{hi} C(len, k)`, where `lo`/`hi` are `worker`'s own
/// bounds (`min_size` floored at 0, `max_size` capped at `len`).
///
/// **This exists so the refusal can happen before the allocation** (AUDIT C124). The enumeration *is*
/// the denial of service — at `MAX_SUBSET_ITEMS` a single dimension is 2¹⁹ pairs of two `Vec`s, and
/// `sub_pars` multiplies seven dimensions — so `sub_pars` has to be able to ask "how big would this
/// be?" and answer it from arithmetic. `debug_assert_eq!`s in `sub_pars` check it against the
/// materialized lengths in every test run, so a drift between the two cannot be silent.
fn subset_count(len: usize, min_size: i32, max_size: i32) -> u64 {
    if max_size < 0 || min_size > max_size {
        return 0;
    }
    let n = len as u64;
    let lo = if min_size <= 0 { 0 } else { min_size as u64 };
    let hi = std::cmp::min(max_size as u64, n);
    if lo > hi {
        return 0;
    }
    (lo..=hi).map(|k| binomial(n, k)).sum()
}

/// `C(n, k)` by the multiplicative formula, saturating. The division is exact at every step when
/// the factors are applied in this order, and a saturated result is a *refusal* upstream rather
/// than a panic — this function runs on peer-supplied deploy data.
fn binomial(n: u64, k: u64) -> u64 {
    let k = std::cmp::min(k, n.saturating_sub(k));
    (1..=k).fold(1u64, |acc, i| acc.saturating_mul(n - k + i) / i)
}

/// Generate every (subset, complement) pair whose subset size is in `[minSize, maxSize]` (port of
/// `minMaxSubsets`).
///
/// Rejects (rather than materializes) enumerations whose input exceeds [`MAX_SUBSET_ITEMS`], so a
/// connective pattern cannot force exponential work.
pub fn min_max_subsets<A: Clone>(
    items: &[A],
    min_size: i32,
    max_size: i32,
) -> Result<Vec<(Vec<A>, Vec<A>)>, RholangError> {
    if items.len() > MAX_SUBSET_ITEMS {
        return Err(RholangError::ReduceError(format!(
            "spatial match subset enumeration too large: {} items exceeds limit {MAX_SUBSET_ITEMS}",
            items.len()
        )));
    }
    Ok(worker(items, min_size, max_size)
        .into_iter()
        .map(|(sub, comp, _)| (sub, comp))
        .collect())
}

fn counted_max_subsets<A: Clone>(items: &[A], max_size: i32) -> Vec<(Vec<A>, Vec<A>, i32)> {
    if items.is_empty() {
        return vec![(Vec::new(), Vec::new(), 0)];
    }
    let head = items[0].clone();
    let rem = &items[1..];
    let mut out = vec![(Vec::new(), items.to_vec(), 0)];
    for (tail, complement, count) in counted_max_subsets(rem, max_size) {
        if count == max_size {
            let mut comp = complement;
            comp.insert(0, head.clone());
            out.push((tail, comp, count));
        } else if tail.is_empty() {
            let mut sub = tail;
            sub.insert(0, head.clone());
            out.push((sub, complement, 1));
        } else {
            let mut comp = complement.clone();
            comp.insert(0, head.clone());
            out.push((tail.clone(), comp, count));
            let mut sub = tail;
            sub.insert(0, head.clone());
            out.push((sub, complement, count + 1));
        }
    }
    out
}

fn worker<A: Clone>(items: &[A], min_size: i32, max_size: i32) -> Vec<(Vec<A>, Vec<A>, i32)> {
    if max_size < 0 || min_size > max_size {
        return Vec::new();
    }
    if min_size <= 0 {
        if max_size == 0 {
            return vec![(Vec::new(), items.to_vec(), 0)];
        }
        return counted_max_subsets(items, max_size);
    }
    if items.is_empty() {
        return Vec::new();
    }
    let head = items[0].clone();
    let rem = &items[1..];
    let decr = min_size - 1;
    let mut out = Vec::new();
    for (tail, complement, count) in worker(rem, decr, max_size) {
        if count == max_size {
            let mut comp = complement;
            comp.insert(0, head.clone());
            out.push((tail, comp, count));
        } else if count == decr {
            let mut sub = tail;
            sub.insert(0, head.clone());
            out.push((sub, complement, min_size));
        } else {
            let mut comp = complement.clone();
            comp.insert(0, head.clone());
            out.push((tail.clone(), comp, count));
            let mut sub = tail;
            sub.insert(0, head.clone());
            out.push((sub, complement, count + 1));
        }
    }
    out
}

/// Split `par` into every (matched sub-`Par`, remainder) pair consistent with the min/max bounds
/// (port of `subPars`).
///
/// The 7-way Cartesian product of the per-dimension subset lists is the exponential blowup a
/// connective pattern can trigger; the total number of splits is capped at
/// [`MAX_SPLIT_COMBINATIONS`] (and each dimension at [`MAX_SUBSET_ITEMS`]).
pub fn sub_pars<S: Sort>(
    par: &Par<S>,
    min: &ParCount,
    max: &ParCount,
    min_prune: &ParCount,
    max_prune: &ParCount,
) -> Result<Vec<(Par<S>, Par<S>)>, RholangError> {
    let send_max = i32::min(max.sends, par.sends.len() as i32 - min_prune.sends);
    let receive_max = i32::min(max.receives, par.receives.len() as i32 - min_prune.receives);
    let news_max = i32::min(max.news, par.news.len() as i32 - min_prune.news);
    let expr_max = i32::min(max.exprs, par.exprs.len() as i32 - min_prune.exprs);
    let match_max = i32::min(max.matches, par.matches.len() as i32 - min_prune.matches);
    let unf_max = i32::min(
        max.unforgeables,
        par.unforgeables.len() as i32 - min_prune.unforgeables,
    );
    let bundle_max = i32::min(max.bundles, par.bundles.len() as i32 - min_prune.bundles);

    let send_min = i32::max(min.sends, par.sends.len() as i32 - max_prune.sends);
    let receive_min = i32::max(min.receives, par.receives.len() as i32 - max_prune.receives);
    let news_min = i32::max(min.news, par.news.len() as i32 - max_prune.news);
    let expr_min = i32::max(min.exprs, par.exprs.len() as i32 - max_prune.exprs);
    let match_min = i32::max(min.matches, par.matches.len() as i32 - max_prune.matches);
    let unf_min = i32::max(
        min.unforgeables,
        par.unforgeables.len() as i32 - max_prune.unforgeables,
    );
    let bundle_min = i32::max(min.bundles, par.bundles.len() as i32 - max_prune.bundles);

    // **Refuse before materialising** (AUDIT C124). The guard inside `min_max_subsets` caps how many
    // *items* a dimension may hold; it does not cap what one dimension *enumerates*, and the product
    // was checked only after all seven dimensions had been built — so a 217-byte deploy that would
    // produce ~16 GB was fully materialized before the check that refuses it could see the total.
    // The counts are arithmetic, so the total is known first, and nothing is allocated when the
    // total is too large.
    let counts = [
        subset_count(par.sends.len(), send_min, send_max),
        subset_count(par.receives.len(), receive_min, receive_max),
        subset_count(par.news.len(), news_min, news_max),
        subset_count(par.exprs.len(), expr_min, expr_max),
        subset_count(par.matches.len(), match_min, match_max),
        subset_count(par.unforgeables.len(), unf_min, unf_max),
        subset_count(par.bundles.len(), bundle_min, bundle_max),
    ];
    let total = counts.iter().fold(1u64, |acc, c| acc.saturating_mul(*c));
    if total > MAX_SPLIT_COMBINATIONS {
        return Err(RholangError::ReduceError(format!(
            "spatial match split too large: {total} combinations exceeds limit {MAX_SPLIT_COMBINATIONS}"
        )));
    }

    let sub_sends = min_max_subsets(&par.sends, send_min, send_max)?;
    let sub_receives = min_max_subsets(&par.receives, receive_min, receive_max)?;
    let sub_news = min_max_subsets(&par.news, news_min, news_max)?;
    let sub_exprs = min_max_subsets(&par.exprs, expr_min, expr_max)?;
    let sub_matches = min_max_subsets(&par.matches, match_min, match_max)?;
    let sub_unfs = min_max_subsets(&par.unforgeables, unf_min, unf_max)?;
    let sub_bundles = min_max_subsets(&par.bundles, bundle_min, bundle_max)?;

    // What refused is the arithmetic above; this is the check that it was *right*. Every test run
    // compares the predicted counts against the materialized lengths, so a drift between
    // `subset_count` and `worker` cannot pass silently — and if it ever did, the drift would be a
    // refusal that is too permissive, which is the failure C124 recorded.
    debug_assert_eq!(
        counts,
        [
            sub_sends.len() as u64,
            sub_receives.len() as u64,
            sub_news.len() as u64,
            sub_exprs.len() as u64,
            sub_matches.len() as u64,
            sub_unfs.len() as u64,
            sub_bundles.len() as u64,
        ],
        "subset_count disagreed with the enumeration it predicts — the bound would be wrong"
    );

    let mut out = Vec::new();
    for ss in &sub_sends {
        for sr in &sub_receives {
            for sn in &sub_news {
                for se in &sub_exprs {
                    for sm in &sub_matches {
                        for su in &sub_unfs {
                            for sb in &sub_bundles {
                                let sub = Par {
                                    sends: ss.0.clone(),
                                    receives: sr.0.clone(),
                                    news: sn.0.clone(),
                                    exprs: se.0.clone(),
                                    matches: sm.0.clone(),
                                    unforgeables: su.0.clone(),
                                    bundles: sb.0.clone(),
                                    ..Default::default()
                                };
                                let comp = Par {
                                    sends: ss.1.clone(),
                                    receives: sr.1.clone(),
                                    news: sn.1.clone(),
                                    exprs: se.1.clone(),
                                    matches: sm.1.clone(),
                                    unforgeables: su.1.clone(),
                                    bundles: sb.1.clone(),
                                    ..Default::default()
                                };
                                out.push((sub, comp));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_max_subsets_rejects_oversized_input() {
        let items: Vec<i32> = (0..(MAX_SUBSET_ITEMS as i32 + 1)).collect();
        assert!(min_max_subsets(&items, 0, items.len() as i32).is_err());
    }

    #[test]
    fn min_max_subsets_enumerates_small_input() {
        let items = vec![1, 2, 3];
        // min=0, max=3: every subset of the 3 items (2^3 = 8).
        let subs = min_max_subsets(&items, 0, 3).unwrap();
        assert_eq!(subs.len(), 8);
    }

    /// **The two bounds have to agree, and today they did not** (AUDIT C124). `MAX_SUBSET_ITEMS` is
    /// what one dimension may hold; `MAX_SPLIT_COMBINATIONS` is what the product may reach. If the
    /// first permits more than the second, the product check can only reject work that has already
    /// been materialized — which is exactly the 16 GB, and why the constant is 19 rather than 20.
    ///
    /// This is the row's own named proof, red on the tree before the fix.
    #[test]
    fn a_single_dimension_cannot_outgrow_the_product_cap() {
        assert!(
            (1u64 << MAX_SUBSET_ITEMS) <= MAX_SPLIT_COMBINATIONS,
            "one dimension at MAX_SUBSET_ITEMS enumerates 2^{MAX_SUBSET_ITEMS} = {} splits, which \
             exceeds MAX_SPLIT_COMBINATIONS = {MAX_SPLIT_COMBINATIONS}: the per-dimension guard \
             would admit work the product guard must then refuse, after it was built",
            1u64 << MAX_SUBSET_ITEMS
        );
    }

    /// `subset_count` is the arithmetic the refusal now rests on, so it is checked against the
    /// enumeration itself across every shape rather than against hand-computed binomials — a
    /// miscount that is too *permissive* is the direction that matters, and only the enumeration
    /// can catch it.
    #[test]
    fn subset_count_predicts_the_enumeration() {
        let items: Vec<i32> = (0..8).collect();
        for len in 0..=8usize {
            for min_size in -1..=3i32 {
                for max_size in -1..=4i32 {
                    let predicted = subset_count(len, min_size, max_size);
                    let actual = min_max_subsets(&items[..len], min_size, max_size)
                        .expect("under the item cap")
                        .len() as u64;
                    assert_eq!(
                        predicted, actual,
                        "subset_count disagreed for len={len} min={min_size} max={max_size}"
                    );
                }
            }
        }
    }

    /// **Twenty items in one dimension is the shape that used to build 2²⁰ pairs before refusing
    /// them.** The count says so, and the enumeration is never asked to try — which is the whole
    /// point of checking from arithmetic.
    #[test]
    fn twenty_items_in_one_dimension_is_refused_by_arithmetic() {
        assert!(
            subset_count(20, 0, 20) > MAX_SPLIT_COMBINATIONS,
            "a 20-item dimension must be refused on its count alone"
        );
        let items: Vec<i32> = (0..20).collect();
        assert!(
            min_max_subsets(&items, 0, 20).is_err(),
            "…and the per-dimension guard must agree with the count"
        );
    }
}
