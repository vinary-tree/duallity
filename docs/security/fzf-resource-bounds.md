# fzf scorer resource boundaries

The user-controlled dimensions are query length, candidate length, dictionary
edge count, dictionary depth, and the top-`` $`k`$ `` heap size.

| Input | Growth mode | Guard |
|---|---|---|
| query length `` $`m`$ `` | every visited edge advances `` $`m`$ `` DP cells | `FzfConfig::max_query_chars` |
| candidate length `` $`n`$ `` | independent scoring performs `` $`O(mn)`$ `` work | `FzfConfig::max_candidate_chars` |
| visited edges `` $`E_v`$ `` | traversal performs `` $`O(mE_v)`$ `` work | caller dictionary and timeout |
| dictionary depth `` $`d`$ `` | active columns use `` $`O(md)`$ `` memory | candidate-length ceiling |
| `top_k` | minimum heap uses `` $`O(k)`$ `` memory | caller configuration |
| lazy WFST states | DAWG joins remain path-sensitive | cache policy and state budget |

The upper bound deliberately retains an unstarted alignment. Removing it would
improve pruning by becoming unsound. For untrusted queries, use smaller limits
than the permissive library defaults, request deadlines, and a service-layer
cap on lazy-state expansion.
