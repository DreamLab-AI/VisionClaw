---
type: Class
title: 51% Attack
resource: urn:ngm:class:51-attack
page_resource: urn:visionflow:page:61986621941769792830c59c2be66e9fd4019ea25ed77ea19408e21d59fa621e
legacy-term-id: BC-9981
public: true
status: stable
domain: blockchain
maturity: emerging
quality: 0.72
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links:
- '[[ai-energy-optimisation|AI Energy Optimisation]]'
- '[[altcoins]]'
- '[[asic|ASIC]]'
- '[[bitcoin-gold|Bitcoin Gold]]'
- '[[blockchain-reorganization|blockchain reorganization]]'
- '[[blockchain-reorganization|Blockchain Reorganization]]'
- '[[blockchain-security|blockchain security]]'
- '[[coinbase|Coinbase]]'
- '[[consensus|Consensus]]'
- '[[consensus]]'
- '[[consensus-attack|Consensus Attack]]'
- '[[consensus-mechanisms|consensus mechanisms]]'
- '[[cryptocurrencies]]'
- '[[cryptographic]]'
- '[[ddo-s-attack|DDoS attack]]'
- '[[decred|Decred]]'
- '[[ethereum-classic|Ethereum Classic]]'
- '[[hash-rate|hash rate]]'
- '[[hash-rate|Hash Rate]]'
- '[[ieee-2418-1|IEEE 2418.1]]'
- '[[iso-iec-23257-2021|ISO/IEC 23257:2021]]'
- '[[majority-attack|Majority attack]]'
- '[[mining-hardware|mining hardware]]'
- '[[mining-pools|mining pools]]'
- '[[monero|Monero]]'
- '[[nice-hash|NiceHash]]'
- '[[nist-cybersecurity-framework|NIST Cybersecurity Framework]]'
- '[[nist-nistir-8202|NIST NISTIR 8202]]'
- '[[polkadot|Polkadot]]'
- '[[private-keys|private keys]]'
- '[[quantum-resistant]]'
- '[[qubic|Qubic]]'
- '[[satoshi-nakamoto|Satoshi Nakamoto]]'
- '[[transaction-censorship|transaction censorship]]'
- '[[transactions]]'
- '[[bitcoin|Bitcoin]]'
- '[[blockchain|Blockchain]]'
- '[[blockchain]]'
- '[[byzantine-fault-tolerance|Byzantine Fault Tolerance]]'
- '[[cardano|Cardano]]'
- '[[cryptocurrency]]'
- '[[double-spending]]'
- '[[double-spending|Double-spending]]'
- '[[double-spending|Double-Spending]]'
- '[[ethereum|Ethereum]]'
- '[[finality|Finality]]'
- '[[mining]]'
- '[[mining-pool|mining pool]]'
- '[[mining-pool|Mining Pool]]'
- '[[proof-of-stake|Proof-of-Stake]]'
- '[[proof-of-work|Proof-of-Work]]'
- '[[selfish-mining|Selfish Mining]]'
- '[[transaction]]'
is-a:
- '[[bc-protocol-and-consensus|Protocol and Consensus]]'
bridges-to:
- '[[network-security|Network Security]]'
- '[[game-theory|Game Theory]]'
- '[[ai-energy-optimisation|AI Energy Optimisation]]'
contrasts-with:
- '[[proof-of-stake|Proof of Stake]]'
- '[[byzantine-fault-tolerance|Byzantine Fault Tolerance]]'
depends-on:
- '[[blockchain|Blockchain]]'
- '[[cryptocurrency|Cryptocurrency]]'
- '[[mining-hardware|Mining Hardware]]'
enables:
- '[[double-spending|Double Spending]]'
- '[[transaction-censorship|Transaction Censorship]]'
- '[[chain-reorganization|Blockchain Reorganization]]'
part-of:
- '[[blockchain-security|Blockchain Security]]'
related-to:
- '[[selfish-mining|Selfish Mining]]'
- '[[finality|Finality]]'
- '[[transaction|Transaction]]'
- '[[ethereum-smart-contract-platform-classic|Ethereum Classic]]'
requires:
- '[[proof-of-work|Proof Of Work]]'
- '[[mining-pool|Mining Pool]]'
- '[[hash-rate|Hash Rate]]'
- '[[mining|Mining]]'
---

A 51% Attack is a consensus-layer attack on a Proof-of-Work blockchain in which a single entity or coalition controls more than half of the network's hash rate, enabling double-spending, transaction censorship, and chain reorganisation. The attack exploits the longest-chain rule: the attacker mines a private fork containing fraudulent transactions and, once it exceeds the honest chain in cumulative work, broadcasts it to override confirmed history.

### Semantic Classification

### Content

51% Attack refers to [[Majority attack]] on [[Proof-of-Work]] [[blockchain]] networks where a single entity or coalition controls more than 50% of the network's [[hash rate]], enabling [[double-spending]], [[transaction censorship]], and [[blockchain reorganization]].

#### Key Characteristics
1. **Majority Control**: Attacker controls >50% of network [[hash rate]]
2. **Chain Reorganization**: Ability to create longer [[blockchain]] forks that override honest chain
3. **Double-Spending**: Can reverse recent [[transactions]] to spend same coins twice
4. **Transaction Censorship**: Can prevent specific [[transactions]] from confirming
5. **Limited Scope**: Cannot forge transactions requiring [[private keys]] or create coins from nothing

### Attack Mechanism
**How It Works**:

- Attacker mines blocks faster than the rest of the network combined
- Creates a private fork of the [[blockchain]] containing fraudulent [[transactions]]
- When private chain becomes longer, broadcasts it to network
- [[Consensus]] rules accept longest chain, orphaning honest blocks
- Previously confirmed [[transactions]] are reversed, enabling [[double-spending]]
  **Technical Requirements**:
- Control of majority [[hash rate]] (>50% of network computational power)
- Specialized [[mining hardware]] (ASICs for most networks)
- Significant electricity costs
- Coordination of [[mining pools]] (if using multiple sources)

  ### Real-World Examples [Updated 2025]

  #### Monero Attack (August 2025)

  **[Updated 2025]** In August 2025, [[Qubic]], a Layer-1 [[blockchain]] designed for computational [[Proof-of-Work]], directed its [[mining pool]] toward attacking [[Monero]]. The operation achieved:

- Six-block deep [[blockchain reorganization]]
- Approximately 60 orphaned blocks
- [[Qubic]] had configured its network to perform Monero's PoW hashing, earning block rewards while executing the attack
- The [[Monero]] community responded with a [[DDoS attack]] targeting [[Qubic]]'s infrastructure, disrupting coordination and halting the attack
  **Significance**: This incident demonstrated that even larger, established [[cryptocurrency]] networks face real threats from well-resourced attackers, moving 51% attacks from theoretical vulnerabilities to practical risks.
  *Source: Halborn Security (2025). "Explained: The Monero 51% Attack"*

  #### Ethereum Classic (Multiple Attacks)

  **[Updated 2025]** [[Ethereum Classic]] (ETC) has been one of the most frequently attacked blockchains:

- **January 2019**: [[Coinbase]] identified a "deep chain reorganization" including [[double-spending]] on January 5, 2019. Coinbase halted all ETC transactions.
- **August 2020**: Massive attack with [[double-spending]] of $5.6 million worth of ETC
- **2020 Series**: Network experienced three additional attacks in 2020, losing over $5 million total
- **2024 Attack**: Further [[double-spending]] incidents and [[transaction]] disruptions, causing financial harm and reputational damage
  **Why Targeted**: Lower [[hash rate]] compared to [[Ethereum Smart Contract Platform]], making it economically feasible to rent sufficient computational power for attacks.
  *Sources: Coinbase Security (2019), BeInCrypto (2024)*

  #### Bitcoin Gold (Ongoing Target)

  **[Updated 2025]** [[Bitcoin Gold]] (BTG) has suffered over 40 detected 51% attacks:

- **May 2018**: First major attack with [[double-spending]] of approximately $18 million worth of BTG
- **January 2020**: Attack on January 23-24 resulted in [[double-spending]] of ~$7,000 worth of BTG with two reorganizations exceeding ten blocks
- **Ongoing Vulnerability**: Continues to be targeted due to relatively low [[hash rate]] and [[ASIC]]-resistant algorithm making rental attacks viable
  **Why Vulnerable**: [[Bitcoin Gold]]'s lower [[hash rate]] and accessibility of compatible [[mining hardware]] through rental services.
  *Sources: 99Bitcoins (2025), CryptoNews Academy*

  ### Economic Analysis

  #### Cost of Attack [Updated 2025]

  **Large Networks (Highly Secure)**:

- [[Bitcoin Proof-of-Work Protocol]]: Hash rate exceeds 600 EH/s (exahashes per second)
- Estimated cost: $20+ billion in hardware, plus ongoing electricity costs exceeding $1 million/day
- **Practically immune** due to prohibitive costs
  **Smaller Networks (Vulnerable)**:
- [[Ethereum Classic]]: ~150 TH/s
- [[Bitcoin Gold]]: ~5 TH/s
- Attack cost: As low as $50,000-$500,000 via [[hash rate]] rental services like [[NiceHash]]
- **Economically feasible** for motivated attackers with potential profits exceeding costs

  #### Attacker Incentives

  1. **Financial Gain**: [[Double-spending]] to defraud exchanges
  2. **Market Manipulation**: Shorting cryptocurrency before attack to profit from price crash
  3. **Competitive Sabotage**: Damaging rival [[blockchain]] networks
  4. **Ideological Motivation**: Proving vulnerabilities in specific networks
  *Source: MIT Digital Currency Initiative (2023), "Economic Incentives and Feasibility of 51% Attacks"*

  ### Prevention and Mitigation Strategies [Updated 2025]

  #### 1. Alternative Consensus Mechanisms

- **[[Proof-of-Stake]] (PoS)**: Replaces [[hash rate]] with token ownership
- Attack cost shifts from hardware to capital
- Requires acquiring >50% of token supply (often billions of dollars)
- Examples: [[Ethereum Smart Contract Platform]] 2.0, [[Cardano]], [[Polkadot]]
- **Hybrid Models**: Combine PoW with PoS or other mechanisms
- [[Decred]]: Hybrid PoW/PoS system
- Makes attacks significantly more complex and expensive

  #### 2. Checkpointing

- Anchors certain blocks in the chain as immutable
- Limits depth of possible [[blockchain reorganization]]
- Makes deep reorganizations computationally infeasible
- **Trade-off**: Reduces flexibility for legitimate forks and upgrades
- **Example**: [[Ethereum Classic]] implemented checkpointing after 2020 attacks

  #### 3. Hash Rate Monitoring

- Real-time monitoring of [[hash rate]] distribution
- Alert systems for sudden spikes in single [[mining pool]] share
- **Best Practice**: No single pool should exceed 25% of network [[hash rate]]
- Tools: Blockchain explorers, mining pool dashboards

  #### 4. Increased Decentralization

- Encourage diverse set of [[mining pools]]
- Geographic distribution of mining operations
- Prevent centralization of [[hash rate]]
- Community governance to identify and address concentration risks

  #### 5. Economic Barriers

- **Staking Requirements**: [[Ethereum Smart Contract Platform]] requires staking 32 ETH (~$54,000+) to become validator
- **Slashing Penalties**: Validators lose stake for malicious behaviour
- **Bonding Mechanisms**: Economic deterrents for attack attempts

  #### 6. Network Upgrades

- Transition to more secure [[consensus]] algorithms
- Implement ASIC-resistant [[mining]] algorithms (with caveats)
- Regular security audits and vulnerability assessments
  *Sources: Hacken (2025), MIT DCI, Unchained (2025)*

  ### Technical Limitations
  **What Attackers CANNOT Do**:

- Forge transactions requiring [[private keys]]
- Create new coins beyond block rewards
- Access or steal users' wallets
- Modify transactions older than the reorganization depth
- Prevent all transactions permanently (network can recover)
  **What Attackers CAN Do**:
- Reverse recent [[transactions]] (typically within last few blocks)
- Execute [[double-spending]] attacks
- Censor specific [[transactions]] or addresses
- Temporarily halt block production
- Create orphaned blocks

  #### Academic Context
  The academic foundation stems from the [[Bitcoin Proof-of-Work Protocol]] whitepaper by [[Satoshi Nakamoto]] (2008), which assumed the improbability of acquiring majority [[hash rate]]. However, subsequent research has developed sophisticated economic models analysing incentives and feasibility of 51% attacks across various [[cryptocurrencies]].
  **Key Research Areas**:
  1. **Economic Game Theory**: Analyzing attacker incentives and rational behaviour
  2. **Selfish Mining**: Related attack strategy where miners withhold blocks
  3. **Hash Rate Rental Markets**: Impact of services like [[NiceHash]] on attack feasibility
  4. **Defence Mechanisms**: Checkpointing, finality gadgets, hybrid consensus
  5. **Detection Systems**: Real-time monitoring and anomaly detection
  **Influential Papers**:

- Nakamoto, S. (2008). "Bitcoin: A Peer-to-Peer Electronic Cash System"
- Eyal, I., & Sirer, E. G. (2014). "Majority is not enough: Bitcoin mining is vulnerable"
- Glasbergen, G.-J., Lovejoy, J., & Ouyang, A. (2023). "Economic Incentives and Feasibility of 51% Attacks on Proof-of-Work Blockchains". MIT Digital Currency Initiative.

  #### UK Context
  **British Contributions**:

- UK academic institutions (Imperial College London, UCL, Cambridge) contribute significantly to [[blockchain security]] research
- Focus areas: Attack detection, prevention mechanisms, economic modelling
- UK government supports [[blockchain]] innovation through Innovate UK funding
  **North England Innovation Hubs**:
- **Manchester**: Blockchain accelerators working on PoW security enhancements
- **Leeds**: FinTech startups developing [[hash rate]] monitoring tools
- **Sheffield**: Cryptographic research on strengthening [[transaction]] finality
  **Regulatory Approach**:
- FCA (Financial Conduct Authority) monitors [[cryptocurrency]] security risks
- Research partnerships between universities and fintech companies
- Simulation environments for testing 51% attack scenarios and defensive strategies

  #### Future Directions
  **Emerging Trends**:
  1. **Hybrid Consensus Protocols**: Combining PoW security with PoS economics
  2. **AI-Driven Detection**: Machine learning for [[hash rate]] anomaly detection
  3. **Cross-Chain Security**: Protocols sharing security across multiple chains
  4. **Quantum Resistance**: Preparing for quantum computing threats to [[cryptographic]] security
  5. **Decentralized Hash Rate Marketplaces**: Reducing centralization in mining
  **Anticipated Challenges**:

- Balancing decentralization with security as [[mining]] becomes more centralised
- Energy consumption concerns while maintaining robust PoW security
- Protecting smaller [[altcoins]] from economically motivated attackers
- Adapting to evolving [[hash rate]] rental market dynamics
  **Research Priorities** [Updated 2025]:
- Developing scalable, energy-efficient [[consensus mechanisms]] resistant to majority control
- Creating comprehensive incident response frameworks for 51% attack recovery
- Studying socio-economic impacts on user trust and market stability
- Investigating [[quantum-resistant]] consensus algorithms
  ---

  #### Standards and References

- **[[ISO/IEC 23257:2021]]** - Blockchain and distributed ledger technologies — Reference architecture
- **[[IEEE 2418.1]]** - Standard for the Framework of Blockchain Use in Internet of Things (IoT)
- **[[NIST NISTIR 8202]]** - Blockchain Technology Overview
- **[[NIST Cybersecurity Framework]]** - Applied to blockchain security

  #### Related Concepts

- [[Blockchain]] - Distributed ledger technology
- [[Proof-of-Work]] - Consensus mechanism vulnerable to 51% attacks
- [[Proof-of-Stake]] - Alternative consensus mechanism with different security model
- [[Hash Rate]] - Measure of computational power in PoW networks
- [[Double-Spending]] - Primary exploit enabled by 51% attacks
- [[Consensus Attack]] - Broader category of blockchain security threats
- [[Mining Pool]] - Coordination of miners that can centralise hash rate
- [[Blockchain Reorganization]] - Technical mechanism exploited in 51% attacks
- [[Selfish Mining]] - Related attack strategy
- [[Byzantine Fault Tolerance]] - Theoretical framework for distributed consensus
- [[Finality]] - Property of blockchain transactions becoming irreversible
  ---

  #### References
  1. Nakamoto, S. (2008). *Bitcoin: A Peer-to-Peer Electronic Cash System*. Available at: https://bitcoin.org/bitcoin.pdf
  2. Glasbergen, G.-J., Lovejoy, J., & Ouyang, A. (2023). *Economic Incentives and Feasibility of 51% Attacks on Proof-of-Work Blockchains*. MIT Digital Currency Initiative. Available at: https://dci.mit.edu/51-attacks
  3. Halborn Security (2025). *Explained: The Monero 51% Attack (August 2025)*. Halborn Blog. Available at: https://www.halborn.com/blog/post/explained-the-monero-51-percent-attack-august-2025
  4. Laliberte, M. (2019). *Cryptocurrencies and the Critical Vulnerability of a 51% Attack*. FinTech Futures. Available at: https://www.fintechfutures.com/blockchain-crypto-digital-assets/cryptocurrencies-and-the-critical-vulnerability-of-a-51-attack
  5. 99Bitcoins. (2025). *51% Attack Explained Simply + Real Life Example (2025 Updated)*. Available at: https://99bitcoins.com/wiki/51-percent-attack/
  6. BeInCrypto. (2024). *51% Attacks on the Blockchain Explained: What Are the Dangers?* Available at: https://beincrypto.com/learn/51-attacks-explained/
  7. Hacken. (2025). *51% Attack: The Concept, Risks & Prevention*. Available at: https://hacken.io/discover/51-percent-attack/
  8. Unchained. (2025). *What Is a 51% Attack in Blockchain?* Available at: https://unchainedcrypto.com/51-percent-attack-in-blockchain/
  9. Eyal, I., & Sirer, E. G. (2014). *Majority is not enough: Bitcoin mining is vulnerable*. In Financial Cryptography and Data Security. Springer, Berlin, Heidelberg.
  10. ISO/IEC 23257:2021. *Blockchain and distributed ledger technologies — Reference architecture*. International Organization for Standardization.
  ---

### Provenance
