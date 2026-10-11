use yams_core::{
    ChunkId, ChunkMetadata, DenseCandidate, ExitCode, GateMode, LexicalScore, PageId, PageLabels,
    PageMetadata, RankError, SearchError, SearchRequest, SearchResponse, SelectionConfig,
    SelectionError, SnippetError, SnippetStatistics, TermFrequency, compose_search,
};

fn page(path: &str) -> PageId {
    PageId::from_canonical_path(path).expect("fictional paths are canonical")
}

#[test]
fn dense_results_are_owned_and_presentation_neutral() {
    let alpha = page("/fictional/shared/alpha.md");
    let beta = page("/fictional/private/beta.md");
    let pages = [
        PageMetadata::new(
            alpha.clone(),
            "alpha",
            PageLabels::new(Some("shared"), None, None),
        ),
        PageMetadata::new(
            beta.clone(),
            "beta",
            PageLabels::new(Some("private"), Some("historical"), None),
        ),
    ];
    let chunks = [
        ChunkMetadata::new(
            ChunkId::new(alpha.clone(), 0),
            "alpha answers the lantern question",
        ),
        ChunkMetadata::new(ChunkId::new(beta.clone(), 0), "beta is unrelated"),
    ];
    let alpha_vector = [1.0, 0.0];
    let beta_vector = [0.8, 0.6];
    let dense = [
        DenseCandidate::new(ChunkId::new(alpha, 0), &alpha_vector),
        DenseCandidate::new(ChunkId::new(beta, 0), &beta_vector),
    ];
    let statistics = SnippetStatistics {
        total_chunks: 2,
        frequencies: vec![
            TermFrequency {
                term: "lantern".into(),
                matching_chunks: 1,
            },
            TermFrequency {
                term: "question".into(),
                matching_chunks: 1,
            },
        ],
    };
    let config = SelectionConfig::new(1, 0.0, 1.0, GateMode::Apply).unwrap();

    assert_eq!(pages[0].id().as_str(), "/fictional/shared/alpha.md");
    assert_eq!(pages[0].name(), "alpha");
    assert_eq!(pages[0].labels().corpus(), Some("shared"));
    assert_eq!(chunks[0].id().ordinal(), 0);
    assert_eq!(chunks[0].text(), "alpha answers the lantern question");

    let response = compose_search(SearchRequest::new(
        "lantern question",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &[],
        &statistics,
        config,
    ))
    .unwrap();

    assert_eq!(response.exit_code(), ExitCode::Ok);
    assert_eq!(response.query(), "lantern question");
    assert_eq!(response.explanation().query(), "lantern question");
    assert_eq!(response.hits().len(), 1);
    let hit = &response.hits()[0];
    assert_eq!(hit.name(), "alpha");
    assert_eq!(hit.path(), "/fictional/shared/alpha.md");
    assert_eq!(hit.score().get(), 1.0);
    assert_eq!(hit.text(), "alpha answers the lantern question");
    assert_eq!(hit.snippet(), "alpha answers the lantern question");
    assert!(!hit.clipped_start());
    assert!(!hit.clipped_end());
    assert_eq!(hit.corpus(), Some("shared"));
    assert_eq!(hit.status(), None);
    assert_eq!(hit.project(), None);
    assert!(!hit.exact());
    assert_eq!(
        response
            .explanation()
            .hit("/fictional/shared/alpha.md")
            .unwrap()
            .dense_rank(),
        Some(1),
    );
}

#[test]
fn nonfinite_bm25_is_a_typed_input_error() {
    let alpha = page("/fictional/shared/alpha.md");
    let chunk_id = ChunkId::new(alpha.clone(), 0);
    let pages = [PageMetadata::new(alpha, "alpha", PageLabels::default())];
    let chunks = [ChunkMetadata::new(chunk_id.clone(), "alpha token")];
    let vector = [1.0, 0.0];
    let dense = [DenseCandidate::new(chunk_id.clone(), &vector)];
    let lexical = [LexicalScore::new(chunk_id.clone(), f64::NAN, 1)];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 1,
        }],
    };

    let error = compose_search(SearchRequest::new(
        "token",
        &vector,
        &pages,
        &chunks,
        &dense,
        &lexical,
        &statistics,
        SelectionConfig::default(),
    ))
    .unwrap_err();

    assert_eq!(error, SearchError::NonFiniteBm25 { chunk: chunk_id });
}

#[test]
fn duplicate_page_metadata_is_refused_before_ranking() {
    let alpha = page("/fictional/shared/alpha.md");
    let pages = [
        PageMetadata::new(alpha.clone(), "first", PageLabels::default()),
        PageMetadata::new(alpha.clone(), "second", PageLabels::default()),
    ];
    let chunk_id = ChunkId::new(alpha.clone(), 0);
    let chunks = [ChunkMetadata::new(chunk_id.clone(), "alpha token")];
    let vector = [1.0, 0.0];
    let dense = [DenseCandidate::new(chunk_id, &vector)];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 1,
        }],
    };

    let error = compose_search(SearchRequest::new(
        "token",
        &vector,
        &pages,
        &chunks,
        &dense,
        &[],
        &statistics,
        SelectionConfig::default(),
    ))
    .unwrap_err();

    assert_eq!(error, SearchError::DuplicatePageMetadata { page: alpha });
}

#[test]
fn duplicate_chunk_and_dense_identities_are_refused() {
    let alpha = page("/fictional/shared/alpha.md");
    let chunk_id = ChunkId::new(alpha.clone(), 0);
    let pages = [PageMetadata::new(alpha, "alpha", PageLabels::default())];
    let duplicate_chunks = [
        ChunkMetadata::new(chunk_id.clone(), "first token"),
        ChunkMetadata::new(chunk_id.clone(), "second token"),
    ];
    let vector = [1.0, 0.0];
    let dense = [DenseCandidate::new(chunk_id.clone(), &vector)];
    let statistics = SnippetStatistics {
        total_chunks: 2,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 2,
        }],
    };

    assert_eq!(
        compose_search(SearchRequest::new(
            "token",
            &vector,
            &pages,
            &duplicate_chunks,
            &dense,
            &[],
            &statistics,
            SelectionConfig::default(),
        )),
        Err(SearchError::DuplicateChunkMetadata {
            chunk: chunk_id.clone(),
        }),
    );

    let chunks = [ChunkMetadata::new(chunk_id.clone(), "token")];
    let duplicate_dense = [
        DenseCandidate::new(chunk_id.clone(), &vector),
        DenseCandidate::new(chunk_id.clone(), &vector),
    ];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 1,
        }],
    };
    assert_eq!(
        compose_search(SearchRequest::new(
            "token",
            &vector,
            &pages,
            &chunks,
            &duplicate_dense,
            &[],
            &statistics,
            SelectionConfig::default(),
        )),
        Err(SearchError::DuplicateDenseCandidate { chunk: chunk_id }),
    );
}

#[test]
fn every_loaded_chunk_has_one_page_and_one_dense_candidate() {
    let alpha = page("/fictional/shared/alpha.md");
    let beta = page("/fictional/shared/beta.md");
    let alpha_id = ChunkId::new(alpha.clone(), 0);
    let beta_id = ChunkId::new(beta.clone(), 0);
    let vector = [1.0, 0.0];
    let stats_one = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 1,
        }],
    };

    let orphan_chunks = [ChunkMetadata::new(beta_id.clone(), "token")];
    let orphan_dense = [DenseCandidate::new(beta_id.clone(), &vector)];
    assert_eq!(
        compose_search(SearchRequest::new(
            "token",
            &vector,
            &[PageMetadata::new(
                alpha.clone(),
                "alpha",
                PageLabels::default(),
            )],
            &orphan_chunks,
            &orphan_dense,
            &[],
            &stats_one,
            SelectionConfig::default(),
        )),
        Err(SearchError::MissingPageMetadata { page: beta.clone() }),
    );

    let pages = [
        PageMetadata::new(alpha, "alpha", PageLabels::default()),
        PageMetadata::new(beta, "beta", PageLabels::default()),
    ];
    let chunks = [
        ChunkMetadata::new(alpha_id.clone(), "alpha token"),
        ChunkMetadata::new(beta_id.clone(), "beta token"),
    ];
    let dense = [DenseCandidate::new(alpha_id, &vector)];
    let stats_two = SnippetStatistics {
        total_chunks: 2,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 2,
        }],
    };
    assert_eq!(
        compose_search(SearchRequest::new(
            "token",
            &vector,
            &pages,
            &chunks,
            &dense,
            &[],
            &stats_two,
            SelectionConfig::default(),
        )),
        Err(SearchError::MissingDenseCandidate { chunk: beta_id }),
    );
}

#[test]
fn lexical_ids_ranks_and_order_are_validated() {
    let alpha = page("/fictional/shared/alpha.md");
    let beta = page("/fictional/shared/beta.md");
    let alpha_id = ChunkId::new(alpha.clone(), 0);
    let beta_id = ChunkId::new(beta.clone(), 0);
    let pages = [
        PageMetadata::new(alpha, "alpha", PageLabels::default()),
        PageMetadata::new(beta, "beta", PageLabels::default()),
    ];
    let chunks = [
        ChunkMetadata::new(alpha_id.clone(), "alpha token"),
        ChunkMetadata::new(beta_id.clone(), "beta token"),
    ];
    let vector = [1.0, 0.0];
    let second_vector = [0.9, 0.1];
    let dense = [
        DenseCandidate::new(alpha_id.clone(), &vector),
        DenseCandidate::new(beta_id.clone(), &second_vector),
    ];
    let statistics = SnippetStatistics {
        total_chunks: 2,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 2,
        }],
    };

    let duplicate = [
        LexicalScore::new(alpha_id.clone(), -1.0, 1),
        LexicalScore::new(alpha_id.clone(), -0.5, 2),
    ];
    assert_eq!(
        compose_search(SearchRequest::new(
            "token",
            &vector,
            &pages,
            &chunks,
            &dense,
            &duplicate,
            &statistics,
            SelectionConfig::default(),
        )),
        Err(SearchError::DuplicateLexicalCandidate {
            chunk: alpha_id.clone(),
        }),
    );

    let bad_rank = [LexicalScore::new(alpha_id.clone(), -1.0, 2)];
    assert_eq!(
        compose_search(SearchRequest::new(
            "token",
            &vector,
            &pages,
            &chunks,
            &dense,
            &bad_rank,
            &statistics,
            SelectionConfig::default(),
        )),
        Err(SearchError::InvalidLexicalRank {
            chunk: alpha_id.clone(),
            expected: 1,
            actual: 2,
        }),
    );

    let unordered = [
        LexicalScore::new(alpha_id.clone(), -0.5, 1),
        LexicalScore::new(beta_id.clone(), -1.0, 2),
    ];
    assert_eq!(
        compose_search(SearchRequest::new(
            "token",
            &vector,
            &pages,
            &chunks,
            &dense,
            &unordered,
            &statistics,
            SelectionConfig::default(),
        )),
        Err(SearchError::LexicalOrder {
            previous: alpha_id,
            current: beta_id,
        }),
    );
}

#[test]
fn snippet_statistics_match_the_loaded_literal_corpus_and_query_terms() {
    let alpha = page("/fictional/shared/alpha.md");
    let id = ChunkId::new(alpha.clone(), 0);
    let pages = [PageMetadata::new(alpha, "alpha", PageLabels::default())];
    let chunks = [ChunkMetadata::new(id.clone(), "lantern token")];
    let vector = [1.0, 0.0];
    let dense = [DenseCandidate::new(id, &vector)];

    let wrong_count = SnippetStatistics {
        total_chunks: 2,
        frequencies: vec![
            TermFrequency {
                term: "lantern".into(),
                matching_chunks: 1,
            },
            TermFrequency {
                term: "token".into(),
                matching_chunks: 1,
            },
        ],
    };
    assert_eq!(
        compose_search(SearchRequest::new(
            "lantern token",
            &vector,
            &pages,
            &chunks,
            &dense,
            &[],
            &wrong_count,
            SelectionConfig::default(),
        )),
        Err(SearchError::SnippetChunkCountMismatch {
            metadata: 1,
            statistics: 2,
        }),
    );

    let wrong_terms = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "lantern".into(),
            matching_chunks: 1,
        }],
    };
    assert_eq!(
        compose_search(SearchRequest::new(
            "lantern token",
            &vector,
            &pages,
            &chunks,
            &dense,
            &[],
            &wrong_terms,
            SelectionConfig::default(),
        )),
        Err(SearchError::SnippetTermsMismatch {
            expected: vec!["lantern".into(), "token".into()],
            actual: vec!["lantern".into()],
        }),
    );
}

#[test]
fn duplicate_snippet_terms_are_refused_even_when_their_values_agree() {
    let alpha = page("/fictional/shared/alpha.md");
    let id = ChunkId::new(alpha.clone(), 0);
    let pages = [PageMetadata::new(alpha, "alpha", PageLabels::default())];
    let chunks = [ChunkMetadata::new(id.clone(), "lantern")];
    let vector = [1.0, 0.0];
    let dense = [DenseCandidate::new(id, &vector)];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![
            TermFrequency {
                term: "lantern".into(),
                matching_chunks: 1,
            },
            TermFrequency {
                term: "LANTERN".into(),
                matching_chunks: 1,
            },
        ],
    };

    assert_eq!(
        compose_search(SearchRequest::new(
            "lantern",
            &vector,
            &pages,
            &chunks,
            &dense,
            &[],
            &statistics,
            SelectionConfig::default(),
        )),
        Err(SearchError::DuplicateSnippetTerm {
            term: "lantern".into(),
        }),
    );
}

#[test]
fn an_empty_corpus_returns_the_bare_empty_result_without_a_gate() {
    let statistics = SnippetStatistics {
        total_chunks: 0,
        frequencies: Vec::new(),
    };

    let response = compose_search(SearchRequest::new(
        "missing lantern",
        &[1.0, 0.0],
        &[],
        &[],
        &[],
        &[],
        &statistics,
        SelectionConfig::default(),
    ))
    .unwrap();

    assert_eq!(response.exit_code(), ExitCode::Empty);
    assert!(response.hits().is_empty());
    assert!(response.explanation().gate().is_none());
    assert!(response.explanation().hits().is_empty());
    assert!(response.explanation().applied());
}

#[test]
fn lexical_page_rank_selects_the_display_chunk_and_survives_in_explain() {
    let alpha = page("/fictional/shared/alpha.md");
    let beta = page("/fictional/shared/beta.md");
    let alpha_dense = ChunkId::new(alpha.clone(), 0);
    let alpha_lexical = ChunkId::new(alpha.clone(), 1);
    let beta_chunk = ChunkId::new(beta.clone(), 0);
    let pages = [
        PageMetadata::new(alpha, "alpha", PageLabels::default()),
        PageMetadata::new(beta, "beta", PageLabels::default()),
    ];
    let chunks = [
        ChunkMetadata::new(alpha_dense.clone(), "semantic head for alpha"),
        ChunkMetadata::new(alpha_lexical.clone(), "the lexical token answer"),
        ChunkMetadata::new(beta_chunk.clone(), "another token mention"),
    ];
    let alpha_dense_vector = [1.0, 0.0];
    let alpha_lexical_vector = [0.99, 0.1];
    let beta_vector = [0.8, 0.6];
    let dense = [
        DenseCandidate::new(alpha_dense, &alpha_dense_vector),
        DenseCandidate::new(alpha_lexical.clone(), &alpha_lexical_vector),
        DenseCandidate::new(beta_chunk.clone(), &beta_vector),
    ];
    let lexical = [
        LexicalScore::new(beta_chunk, -2.0, 1),
        LexicalScore::new(alpha_lexical, -1.0, 2),
    ];
    assert_eq!(lexical[0].id().ordinal(), 0);
    assert_eq!(lexical[0].bm25(), -2.0);
    assert_eq!(lexical[0].rank(), 1);
    let statistics = SnippetStatistics {
        total_chunks: 3,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 2,
        }],
    };
    let config = SelectionConfig::new(2, 0.0, 1.0, GateMode::Apply).unwrap();

    let response = compose_search(SearchRequest::new(
        "token",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &lexical,
        &statistics,
        config,
    ))
    .unwrap();

    let hit = &response.hits()[0];
    assert_eq!(hit.name(), "alpha");
    assert_eq!(hit.text(), "the lexical token answer");
    let explain = response.explanation().hit(hit.path()).unwrap();
    assert_eq!(explain.dense_rank(), Some(1));
    assert_eq!(explain.bm25_rank(), Some(2));
    assert!(explain.rrf_score().is_some());
    assert_eq!(explain.contributions().len(), 2);
    assert_eq!(explain.contributions()[1].rank(), 2);
}

#[test]
fn a_weak_gate_returns_unsure_but_explains_the_suppressed_candidate() {
    let alpha = page("/fictional/shared/alpha.md");
    let id = ChunkId::new(alpha.clone(), 0);
    let pages = [PageMetadata::new(alpha, "alpha", PageLabels::default())];
    let chunks = [ChunkMetadata::new(id.clone(), "weak lantern")];
    let vector = [0.6, 0.8];
    let dense = [DenseCandidate::new(id, &vector)];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "lantern".into(),
            matching_chunks: 1,
        }],
    };

    let response = compose_search(SearchRequest::new(
        "lantern",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &[],
        &statistics,
        SelectionConfig::default(),
    ))
    .unwrap();

    assert_eq!(response.exit_code(), ExitCode::Unsure);
    assert!(response.hits().is_empty());
    assert!(response.explanation().applied());
    assert!(
        response
            .explanation()
            .hit("/fictional/shared/alpha.md")
            .is_some(),
        "rank signals remain available for a page the applied gate suppresses",
    );
    let gate = response.explanation().gate().unwrap();
    assert!(gate.floor_fired());
    assert_eq!(gate.margin(), -0.12);
    assert_eq!(gate.floor_dropped()[0].path(), "/fictional/shared/alpha.md");
}

#[test]
fn bypass_returns_weak_hits_with_a_hypothetical_gate_verdict() {
    let alpha = page("/fictional/shared/alpha.md");
    let id = ChunkId::new(alpha.clone(), 0);
    let pages = [PageMetadata::new(alpha, "alpha", PageLabels::default())];
    let chunks = [ChunkMetadata::new(id.clone(), "weak lantern")];
    let vector = [0.6, 0.8];
    let dense = [DenseCandidate::new(id, &vector)];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "lantern".into(),
            matching_chunks: 1,
        }],
    };
    let config = SelectionConfig::new(5, 0.72, 0.05, GateMode::Bypass).unwrap();

    let response = compose_search(SearchRequest::new(
        "lantern",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &[],
        &statistics,
        config,
    ))
    .unwrap();

    assert_eq!(response.exit_code(), ExitCode::Ok);
    assert_eq!(response.hits().len(), 1);
    assert!(!response.explanation().applied());
    assert!(response.explanation().gate().unwrap().floor_fired());
}

#[test]
fn snippets_are_additive_and_never_replace_the_full_selected_text() {
    let alpha = page("/fictional/shared/alpha.md");
    let id = ChunkId::new(alpha.clone(), 0);
    let pages = [PageMetadata::new(alpha, "alpha", PageLabels::default())];
    let text = format!(
        "{} TARGET_TOKEN closes the answer",
        "head filler ".repeat(40)
    );
    let chunks = [ChunkMetadata::new(id.clone(), text.clone())];
    let vector = [1.0, 0.0];
    let dense = [DenseCandidate::new(id, &vector)];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: "target_token".into(),
            matching_chunks: 0,
        }],
    };

    let response = compose_search(SearchRequest::new(
        "TARGET_TOKEN",
        &vector,
        &pages,
        &chunks,
        &dense,
        &[],
        &statistics,
        SelectionConfig::default(),
    ))
    .unwrap();

    let hit = &response.hits()[0];
    assert_eq!(hit.text(), text);
    assert!(hit.snippet().contains("TARGET_TOKEN"));
    assert!(hit.clipped_start());
    assert!(
        hit.clipped_end(),
        "the snapped window honestly omits the tail"
    );
}

#[test]
fn a_literal_beyond_the_fusion_pool_replaces_only_the_last_requested_slot() {
    let mut pages = Vec::new();
    let mut chunks = Vec::new();
    let mut ids = Vec::new();
    let mut vectors = Vec::new();
    for index in 0..27_u32 {
        let is_literal = index == 26;
        let name = if is_literal {
            "literal".to_owned()
        } else {
            format!("dense-{index:02}")
        };
        let page = page(&format!("/fictional/shared/{name}.md"));
        let id = ChunkId::new(page.clone(), 0);
        let cosine = 0.99_f32 - index as f32 * 0.01;
        vectors.push([cosine, (1.0 - cosine * cosine).sqrt()]);
        pages.push(PageMetadata::new(page, name, PageLabels::default()));
        chunks.push(ChunkMetadata::new(
            id.clone(),
            if is_literal {
                "the unique ORBIT_ID_7 answer"
            } else {
                "ordinary orbital prose"
            },
        ));
        ids.push(id);
    }
    let dense = ids
        .iter()
        .zip(&vectors)
        .map(|(id, vector)| DenseCandidate::new(id.clone(), vector))
        .collect::<Vec<_>>();
    let lexical = [LexicalScore::new(ids[26].clone(), -3.0, 1)];
    let statistics = SnippetStatistics {
        total_chunks: 27,
        frequencies: vec![TermFrequency {
            term: "orbit_id_7".into(),
            matching_chunks: 1,
        }],
    };
    let config = SelectionConfig::new(5, 0.0, 1.0, GateMode::Apply).unwrap();

    let response = compose_search(SearchRequest::new(
        "ORBIT_ID_7",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &lexical,
        &statistics,
        config,
    ))
    .unwrap();

    assert_eq!(
        response
            .hits()
            .iter()
            .map(|hit| hit.name())
            .collect::<Vec<_>>(),
        ["dense-00", "dense-01", "dense-02", "dense-03", "literal"],
    );
    let literal = response.hits().last().unwrap();
    assert!(literal.exact());
    assert_eq!(literal.text(), "the unique ORBIT_ID_7 answer");
    let explain = response.explanation().hit(literal.path()).unwrap();
    assert_eq!(explain.dense_rank(), Some(27));
    assert_eq!(explain.bm25_rank(), Some(1));
    assert_eq!(explain.contributions().len(), 1);
}

#[test]
fn duplicate_names_remain_distinct_and_optional_labels_stay_raw() {
    let shared = page("/fictional/shared/twin.md");
    let private = page("/fictional/private/twin.md");
    let shared_id = ChunkId::new(shared.clone(), 0);
    let private_id = ChunkId::new(private.clone(), 0);
    let pages = [
        PageMetadata::new(
            shared,
            "twin",
            PageLabels::new(Some("shared"), None, Some("project-a")),
        ),
        PageMetadata::new(
            private,
            "twin",
            PageLabels::new(Some("private"), Some("historical\u{1b}[31m"), None),
        ),
    ];
    let chunks = [
        ChunkMetadata::new(shared_id.clone(), "shared twin token"),
        ChunkMetadata::new(private_id.clone(), "private twin token"),
    ];
    let shared_vector = [1.0, 0.0];
    let private_vector = [0.9, 0.1];
    let dense = [
        DenseCandidate::new(shared_id, &shared_vector),
        DenseCandidate::new(private_id, &private_vector),
    ];
    let statistics = SnippetStatistics {
        total_chunks: 2,
        frequencies: vec![TermFrequency {
            term: "token".into(),
            matching_chunks: 2,
        }],
    };
    let config = SelectionConfig::new(5, 0.0, 1.0, GateMode::Apply).unwrap();

    let response = compose_search(SearchRequest::new(
        "token",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &[],
        &statistics,
        config,
    ))
    .unwrap();

    assert_eq!(response.hits().len(), 2);
    assert_eq!(response.hits()[0].name(), "twin");
    assert_eq!(response.hits()[1].name(), "twin");
    assert_ne!(response.hits()[0].path(), response.hits()[1].path());
    assert_eq!(response.hits()[0].corpus(), Some("shared"));
    assert_eq!(response.hits()[0].status(), None);
    assert_eq!(response.hits()[0].project(), Some("project-a"));
    assert_eq!(response.hits()[1].corpus(), Some("private"));
    assert_eq!(response.hits()[1].status(), Some("historical\u{1b}[31m"));
    assert_eq!(response.hits()[1].project(), None);
}

#[test]
fn debug_output_never_discloses_queries_bodies_snippets_labels_or_paths() {
    const PATH_SECRET: &str = "PATH_SENTINEL_7c91";
    const NAME_SECRET: &str = "NAME_SENTINEL_82de";
    const QUERY_SECRET: &str = "QUERY_SENTINEL_1f4a";
    const BODY_SECRET: &str = "BODY_SENTINEL_4bb3";
    const LABEL_SECRET: &str = "LABEL_SENTINEL_95ac";
    const TERM_SECRET: &str = "TERM_SENTINEL_66d0";

    let page_id = page(&format!("/fictional/private/{PATH_SECRET}.md"));
    let chunk_id = ChunkId::new(page_id.clone(), 0);
    let page_metadata = PageMetadata::new(
        page_id,
        NAME_SECRET,
        PageLabels::new(Some(LABEL_SECRET), Some(LABEL_SECRET), Some(LABEL_SECRET)),
    );
    let chunk_metadata = ChunkMetadata::new(chunk_id.clone(), BODY_SECRET);
    let lexical = LexicalScore::new(chunk_id.clone(), -1.0, 1);
    let vector = [1.0, 0.0];
    let dense = [DenseCandidate::new(chunk_id.clone(), &vector)];
    let pages = [page_metadata.clone()];
    let chunks = [chunk_metadata.clone()];
    let lexical_scores = [lexical.clone()];
    let statistics = SnippetStatistics {
        total_chunks: 1,
        frequencies: vec![TermFrequency {
            term: QUERY_SECRET.to_ascii_lowercase(),
            matching_chunks: 0,
        }],
    };
    let response = compose_search(SearchRequest::new(
        QUERY_SECRET,
        &vector,
        &pages,
        &chunks,
        &dense,
        &lexical_scores,
        &statistics,
        SelectionConfig::default(),
    ))
    .unwrap();
    let errors = [
        SearchError::MissingChunkMetadata {
            chunk: chunk_id.clone(),
        },
        SearchError::SnippetTermsMismatch {
            expected: vec![TERM_SECRET.into()],
            actual: vec![TERM_SECRET.into()],
        },
        SearchError::Selection(SelectionError::InvalidRankSignal {
            path: TERM_SECRET.into(),
        }),
        SearchError::Snippet(SnippetError::InvalidTerm {
            term: TERM_SECRET.into(),
        }),
        SearchError::Rank(RankError::NonCanonicalPagePath {
            path: std::path::PathBuf::from(TERM_SECRET),
        }),
    ];
    let rendered = [
        format!("{page_metadata:?}"),
        format!("{chunk_metadata:?}"),
        format!("{lexical:?}"),
        format!("{:?}", response.hits()[0]),
        format!("{:?}", response.explanation()),
        format!("{response:?}"),
        format!("{:?}", Err::<SearchResponse, _>(errors[0].clone())),
        format!("{errors:?}"),
    ]
    .join("\n");

    for secret in [
        PATH_SECRET,
        NAME_SECRET,
        QUERY_SECRET,
        BODY_SECRET,
        LABEL_SECRET,
        TERM_SECRET,
    ] {
        assert!(
            !rendered.contains(secret),
            "Debug output disclosed private sentinel {secret}: {rendered}",
        );
    }
    assert!(rendered.contains("text_bytes"));
    assert!(rendered.contains("hit_count"));
}

// All paths, bodies and vectors in these tests are synthetic.
fn supersession_search(
    relations: &[(&str, &str, Option<&str>, [f32; 2])],
    limit: usize,
    min_score: f64,
) -> SearchResponse {
    let pages = relations
        .iter()
        .map(|(slug, status, replacement, _)| {
            PageMetadata::new(
                page(&format!("/fictional/pages/{slug}.md")),
                *slug,
                PageLabels::new(Some("shared"), Some(status), None),
            )
            .with_superseded_by(replacement.map(str::to_owned))
        })
        .collect::<Vec<_>>();
    let chunks = pages
        .iter()
        .map(|page| {
            ChunkMetadata::new(
                ChunkId::new(page.id().clone(), 0),
                format!("fictional {} evidence", page.name()),
            )
        })
        .collect::<Vec<_>>();
    let dense = chunks
        .iter()
        .zip(relations)
        .map(|(chunk, (_, _, _, vector))| DenseCandidate::new(chunk.id().clone(), vector))
        .collect::<Vec<_>>();
    let statistics = SnippetStatistics {
        total_chunks: chunks.len() as u64,
        frequencies: vec![TermFrequency {
            term: "evidence".into(),
            matching_chunks: chunks.len() as u64,
        }],
    };
    compose_search(SearchRequest::new(
        "evidence",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &[],
        &statistics,
        SelectionConfig::new(limit, min_score, 1.0, GateMode::Apply).unwrap(),
    ))
    .unwrap()
}

#[test]
fn supersession_promotes_a_lower_scoring_replacement_and_preserves_provenance() {
    let relation = [
        ("old", "historical", Some("new"), [1.0, 0.0]),
        ("new", "current", None, [0.0, 1.0]),
    ];
    let response = supersession_search(&relation, 1, 0.72);
    let hit = &response.hits()[0];
    assert_eq!(hit.name(), "new");
    assert_eq!(hit.text(), "fictional new evidence");
    assert_eq!(hit.score().get(), 0.0);
    assert!(!hit.exact());
    assert_eq!(hit.supersedes(), &["/fictional/pages/old.md"]);
    assert_eq!(
        response.explanation().hit(hit.path()).unwrap().dense_rank(),
        Some(2)
    );
    // With room and the gate bypassed by low thresholds, the old page remains history.
    let response = supersession_search(&relation, 2, 0.0);
    assert_eq!(
        response
            .hits()
            .iter()
            .map(|hit| hit.name())
            .collect::<Vec<_>>(),
        ["new", "old"]
    );
    assert_eq!(
        response.hits()[1].superseded_by(),
        Some("/fictional/pages/new.md")
    );
}

#[test]
fn replacement_chains_deduplicate_and_broken_chains_keep_the_historical_match() {
    let chain = [
        ("old", "historical", Some("middle"), [1.0, 0.0]),
        ("middle", "historical", Some("new"), [0.9, 0.1]),
        ("new", "current", None, [0.0, 1.0]),
    ];
    let response = supersession_search(&chain, 3, 0.0);
    assert_eq!(
        response
            .hits()
            .iter()
            .map(|hit| hit.name())
            .collect::<Vec<_>>(),
        ["new", "old", "middle"]
    );
    assert_eq!(response.hits()[0].supersedes().len(), 2);
    for relations in [
        vec![("old", "historical", Some("missing"), [1.0, 0.0])],
        vec![
            ("old", "historical", Some("other"), [1.0, 0.0]),
            ("other", "historical", Some("old"), [0.0, 1.0]),
        ],
        vec![
            ("old", "historical", Some("other"), [1.0, 0.0]),
            ("other", "in-progress", None, [0.0, 1.0]),
        ],
        vec![
            ("old", "current", Some("other"), [1.0, 0.0]),
            ("other", "current", None, [0.0, 1.0]),
        ],
    ] {
        let response = supersession_search(&relations, 1, 0.0);
        assert_eq!(response.hits()[0].name(), "old");
        assert!(response.hits()[0].supersedes().is_empty());
    }
}

#[test]
fn supersession_does_not_bypass_the_original_match_gate() {
    let response = supersession_search(
        &[
            ("old", "historical", Some("new"), [0.0, 1.0]),
            ("new", "current", None, [0.0, 1.0]),
        ],
        1,
        0.72,
    );
    assert_eq!(response.exit_code(), ExitCode::Unsure);
    assert!(response.hits().is_empty());
}

#[test]
fn replacement_outside_hybrid_candidate_window_can_be_promoted() {
    let mut rows = vec![("old".to_owned(), "historical", Some("new"), [1.0, 0.0])];
    for n in 0..30 {
        rows.push((format!("unrelated-{n}"), "current", None, [0.9, 0.1]));
    }
    rows.push(("new".into(), "current", None, [0.0, 1.0]));
    let rows = rows
        .iter()
        .map(|(slug, status, replacement, vector)| (slug.as_str(), *status, *replacement, *vector))
        .collect::<Vec<_>>();
    assert_eq!(supersession_search(&rows, 1, 0.72).hits()[0].name(), "new");
}

#[test]
fn promoted_replacement_keeps_available_bm25_and_fusion_explanations() {
    let old = page("/fictional/pages/old.md");
    let new = page("/fictional/pages/new.md");
    let pages = [
        PageMetadata::new(
            old.clone(),
            "old",
            PageLabels::new(Some("shared"), Some("historical"), None),
        )
        .with_superseded_by(Some("new".into())),
        PageMetadata::new(
            new.clone(),
            "new",
            PageLabels::new(Some("shared"), Some("current"), None),
        ),
    ];
    let old_chunk = ChunkId::new(old, 0);
    let new_chunk = ChunkId::new(new.clone(), 0);
    let chunks = [
        ChunkMetadata::new(old_chunk.clone(), "old evidence"),
        ChunkMetadata::new(new_chunk.clone(), "new evidence"),
    ];
    let dense = [
        DenseCandidate::new(old_chunk.clone(), &[1.0, 0.0]),
        DenseCandidate::new(new_chunk.clone(), &[0.0, 1.0]),
    ];
    let lexical = [
        LexicalScore::new(old_chunk, -2.0, 1),
        LexicalScore::new(new_chunk, -1.0, 2),
    ];
    let stats = SnippetStatistics {
        total_chunks: 2,
        frequencies: vec![TermFrequency {
            term: "evidence".into(),
            matching_chunks: 2,
        }],
    };
    let response = compose_search(SearchRequest::new(
        "evidence",
        &[1.0, 0.0],
        &pages,
        &chunks,
        &dense,
        &lexical,
        &stats,
        SelectionConfig::new(1, 0.72, 1.0, GateMode::Apply).unwrap(),
    ))
    .unwrap();
    assert_eq!(response.hits()[0].name(), "new");
    let explanation = response.explanation().hit(new.as_str()).unwrap();
    assert_eq!(explanation.dense_rank(), Some(2));
    assert_eq!(explanation.bm25_rank(), Some(2));
    assert!(explanation.rrf_score().is_some());
    assert_eq!(explanation.contributions().len(), 2);
}
