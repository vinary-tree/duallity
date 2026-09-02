# fzf scorer resource boundaries

The user-controlled dimensions are query length, candidate length, dictionary
edge count, dictionary depth, and the top-$`k`$ heap size.

| Input | Growth mode | Guard |
|---|---|---|
| query length $`m`$ | every visited edge advances $`m`$ DP cells | `FzfConfig::max_query_chars` |
| candidate length $`n`$ | independent scoring performs $`\mathcal{O}(mn)`$ work | `FzfConfig::max_candidate_chars` |
| visited edges $`E_v`$ | traversal performs $`\mathcal{O}(mE_v)`$ work | caller dictionary and timeout |
| dictionary depth $`d`$ | active columns use $`\mathcal{O}(md)`$ memory | candidate-length ceiling |
| `top_k` | minimum heap uses $`\mathcal{O}(k)`$ memory | caller configuration |
| lazy WFST states | DAWG joins remain path-sensitive | cache policy and state budget |

The upper bound retains an unstarted alignment only while the remaining
candidate budget can contain the whole query. Removing it earlier would improve
pruning by becoming unsound; configuring a ceiling larger than the real
ingestion limit is safe but needlessly weak. `None` from
`current_upper_bound()` means no live, completed, or unstarted alternative can
finish within the configured limit. For untrusted queries, use truthful limits
smaller than the permissive library defaults, request deadlines, and a
service-layer cap on lazy-state expansion. Monitor score-bound and length-bound
pruning separately so an apparent optimization is not actually rejection at a
resource limit.
