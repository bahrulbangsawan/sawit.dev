//! Loading pages: the page with its count behind it on the same connection, the next page
//! fetched ahead, a page asked for while another loads, cancelling, and the page size.

use futures::channel::oneshot;
use gpui_kit::component::table::TableState;
use gpui_kit::*;
use sawit_core::db::{self, Conn, ResultSet};

use super::{ResultDelegate, TableView};
use crate::rt;

impl TableView {
    /// Re-counts and reloads the current page.
    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pager.total = None;
        self.counted = false;
        self.order_by = None;
        self.prefetched = None;
        // A reload wants fresh data, not the result of a load started before.
        self.in_flight = None;
        self.queued = None;
        self.load_page(self.pager.page, window, cx);
    }

    /// The live read-only client, or `None` after asking the store to
    /// reconnect (the view reloads itself when that finishes).
    fn client(&mut self, cx: &mut Context<Self>) -> Option<Conn> {
        let client = self.db.read(cx).client(&self.profile, true);
        if client.is_none() {
            self.waiting_reconnect = true;
            let profile = self.profile.clone();
            self.db
                .update(cx, |s, cx| s.ensure_connecting(&profile, cx));
        }
        client
    }

    fn load_page(&mut self, page: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((p, set)) = self.prefetched.take()
            && p == page
            && self.error.is_none()
        {
            // Supersedes anything in flight or queued.
            self.generation += 1;
            self.queued = None;
            self.cancel = None;
            self.loading = false;
            self.pager.page = page;
            self.show(set, window, cx);
            self.prefetch(cx);
            cx.notify();
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();
        if self.in_flight.is_some() {
            // Cancelling the old query could race and cancel this one (the
            // server cancels whatever runs on the connection): wait instead.
            self.queued = Some(page);
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        let Some(client) = self.client(cx) else {
            return;
        };
        self.in_flight = Some(generation);
        self.cancel = client.cancel_token();
        let (schema, table) = (self.schema.clone(), self.table.clone());
        let dead = client.clone();
        let cached = self
            .db
            .read(cx)
            .relation(&self.profile, &schema, &table)
            .map(|r| (r.primary_key.clone(), r.estimated_rows));
        let known_order = self
            .order_by
            .clone()
            .or(cached.as_ref().map(|c| c.0.clone()));
        let estimate = cached.and_then(|c| c.1);
        let count = self.pager.total.is_none() && !self.counted;
        self.counted = true;
        let query = self.query.clone();
        let size = self.pager.size;
        let (page_tx, page_rx) = oneshot::channel();
        let count_rx = rt::spawn(cx, async move {
            let pipelined = known_order.is_some();
            let page_done = async {
                let result = async {
                    let order = match known_order {
                        Some(o) => o,
                        None => client.primary_key(&schema, &table).await?,
                    };
                    let set = client
                        .fetch_rows(&schema, &table, &order, &query, page * size, size)
                        .await?;
                    Ok::<_, sawit_core::CoreError>((order, set))
                }
                .await;
                let ok = result.is_ok();
                let _ = page_tx.send(result);
                ok
            };
            let counted = async {
                match count {
                    // The estimate says nothing about a filtered subset.
                    true if query.is_filtered() => client
                        .row_count_matching(&schema, &table, &query)
                        .await
                        .ok()
                        .flatten(),
                    true => client
                        .row_count_estimated(&schema, &table, estimate)
                        .await
                        .ok(),
                    false => None,
                }
            };
            // The page is sent first, so the server answers it before
            // starting the (possibly slow) count queued behind it.
            if pipelined {
                futures::join!(page_done, counted).1
            } else if page_done.await {
                counted.await
            } else {
                None
            }
        });
        cx.spawn_in(window, async move |this, window| {
            let result = page_rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                if this.in_flight == Some(generation) {
                    this.in_flight = None;
                    if let Some(next) = this.queued.take()
                        && next != page
                    {
                        this.load_page(next, window, cx);
                        return;
                    }
                }
                if this.generation != generation {
                    return;
                }
                this.cancel = None;
                if dead.is_closed() {
                    // Dropped mid-request: reconnect and load again.
                    this.pager.total = None;
                    this.counted = false;
                    this.order_by = None;
                    if this.client(cx).is_none() {
                        cx.notify();
                        return;
                    }
                }
                this.loading = false;
                match result {
                    Ok(Ok((order, set))) => {
                        this.order_by = Some(order);
                        this.pager.page = page;
                        this.show(set, window, cx);
                        this.prefetch(cx);
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {
                        this.error = Some("The query stopped unexpectedly. Try again.".into())
                    }
                }
                cx.notify();
            });
            if let Ok(Some(total)) = count_rx.await {
                let _ = this.update(window, |this, cx| {
                    if this.pager.total.is_none() {
                        this.pager.total = Some(total);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Fetches the next page in the background so paging forward is instant.
    fn prefetch(&mut self, cx: &mut Context<Self>) {
        let next = self.pager.page + 1;
        let (Some(order), true) = (self.order_by.clone(), self.pager.has_next()) else {
            return;
        };
        let Some(client) = self.db.read(cx).client(&self.profile, true) else {
            return;
        };
        let generation = self.generation;
        let (schema, table, query) = (self.schema.clone(), self.table.clone(), self.query.clone());
        let size = self.pager.size;
        let rx = rt::spawn(cx, async move {
            client
                .fetch_rows(&schema, &table, &order, &query, next * size, size)
                .await
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(set)) = rx.await {
                let _ = this.update(cx, |this, _| {
                    if this.generation == generation {
                        this.prefetched = Some((next, set));
                    }
                });
            }
        })
        .detach();
    }

    fn show(&mut self, set: ResultSet, window: &mut Window, cx: &mut Context<Self>) {
        self.pager.page_rows = set.rows.len();
        if !set.columns.is_empty() {
            self.columns = set.columns.clone();
        }
        let first_row = self.pager.offset();
        let key = self.key(cx);
        let view = cx.entity().downgrade();
        let mut delegate = ResultDelegate::numbered(set, first_row)
            .sortable(self.query.sort.clone(), view.clone());
        if let Some(key) = key {
            delegate = delegate.pickable(&key, view);
        }
        self.shown_keys = delegate.page_keys();
        self.grid = Some(cx.new(|cx| TableState::new(delegate, window, cx)));
    }

    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.queued = None;
        if let Some(token) = self.cancel.take()
            && let Some(profile) = self.db.read(cx).profile(&self.profile).cloned()
        {
            // Fire and forget: the page request itself reports the outcome.
            drop(rt::spawn(
                cx,
                async move { db::cancel(&token, &profile).await },
            ));
        }
    }

    pub(super) fn go(&mut self, page: u64, window: &mut Window, cx: &mut Context<Self>) {
        if page != self.pager.page || self.error.is_some() {
            self.load_page(page, window, cx);
        }
    }

    /// Shows `size` rows a page, from the page holding the row that was first on the page shown.
    /// The ticked rows stay ticked: they go by key, not by page.
    pub(super) fn set_page_size(&mut self, size: u64, window: &mut Window, cx: &mut Context<Self>) {
        if size == self.pager.size {
            return;
        }
        self.pager = self.pager.resized(size);
        // The page fetched ahead, and one loading, are the old size's.
        self.prefetched = None;
        self.in_flight = None;
        self.queued = None;
        self.load_page(self.pager.page, window, cx);
    }
}
