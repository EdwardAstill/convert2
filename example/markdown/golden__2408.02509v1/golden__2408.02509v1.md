<!-- page:1 -->
# Practical Attacks against Black-box Code Completion Engines

arXiv:2408.02509v1  [cs.CR]  5 Aug 2024

Slobodan Jenko, Jingxuan He, Niels Mu¨ndler, Mark Vero, and Martin Vechev
ETH Zurich, Switzerland
sjenko@ethz.ch, {jingxuan.he, niels.muendler, mark.vero, martin.vechev}@inf.ethz.ch

## Abstract

Modern code completion engines, powered by large
language models, have demonstrated impressive capabilities to
generate functionally correct code based on surrounding context.
As these tools are extensively used by millions of developers, it
is crucial to investigate their security implications. In this work,
we present INSEC, a novel attack that directs code completion
engines towards generating vulnerable code. In line with most
commercial completion engines, such as GitHub Copilot, INSEC
assumes only black-box query access to the targeted engine,
without requiring any knowledge of the engine’s internals. Our
attack works by inserting a malicious attack string as a short
comment in the completion input. To derive the attack string,
we design a series of specialized initialization schemes and an
optimization procedure for further refinement. We demonstrate
the strength of INSEC not only on state-of-the-art open-source
models but also on black-box commercial services such as
the OpenAI API and GitHub Copilot. On a comprehensive
set of security-critical test cases covering 16 CWEs across
5 programming languages, INSEC significantly increases the
likelihood of the considered completion engines in generating
unsafe code by >50% in absolute, while maintaining the ability
in producing functionally correct code. At the same time, our
attack has low resource requirements, and can be developed
for a cost of well under ten USD on commodity hardware.

1. 1. Introduction

Code completion aims to speed up the programming pro-
cess by suggesting fitting code fragments to developers [16],
[25]. Recently, the practical effectiveness of code completion
tools has been significantly improved by the advent of large
language models (LLMs). Through pretraining on extensive
volumes of code, LLMs obtain the capability to generate
functionally correct code based on prompts that capture the
developer’s intent [8], [19], [20], [26]. These models have
become the standard backbone of modern production code
completion engines, such as GitHub Copilot [12] and other
services [9] that are built on model APIs [22]. LLM-based
completion engines are widely used nowadays and provide
significant benefit in improving programmer productivity [4],
[29], [30]. Notably, Copilot is used by more than one million
developers and fvie thousand businesses [10].
Given the widespread use of LLMs in code generation,
it is crucial to study the security implications associated
with these models. Prior research has shown that LLMs
often produce code containing dangerous security vulnera-

bilities even under normal use cases [19], [23], [24]. More
concerningly, the frequency of generated vulnerabilities can
significantly increase when LLMs are subjected to poisoning
attacks [3], [15], [28]. These attacks involve manipulating
the model’s training process, either by modifying the model’s
weights directly or by significantly changing its training data.
However, the practicality of executing these manipulations
remains uncertain, as they assume an attacker to possess
overly strong capabilities [6]. Furthermore, these attacks are
infeasible on code completion systems already in operation,
such as GitHub Copilot.

Our Practical Threat ModelIn this work, we consider
a practical threat model, capturing possible attacks against
current production completion services. Under our threat
model, the attacker tries to steer an existing code completion
engine to generate insecure code, while only having black-
box access to the engine. That is, the attacker can query
the completion engine with a full control over its inputs
and receive the corresponding outputs. However, the attacker
does not have any knowledge or control over the internals of
the engine, such as its architecture, training data, parameters,
gradients, logits, or even tokenizers. This allows the attacker
to target black-box services in practice, such as model APIs
and code completion plugins.
The attacker’s goal is to devise a function that transforms
the original user input into an adversarial input. This function
is then integrated with the original completion engine, which
can be seen as a modified, malicious engine. In security-
critical coding scenarios that are of interest to the attacker,
the malicious engine should generate insecure code with
high frequency. Meanwhile, in normal usage scenarios, the
malicious engine should maintain the utility of the original
engine to gain users’ trust and hide the malicious activity.

Key ChallengesTo construct an effective attack that
complies with our threat model outlined above, the attacker
is faced with two key challenges. First, the attacker needs to
modify the engine’s output behaviors to increase vulnerability,
while simultaneously ensuring that the malicious engine’s
behavior closely mirrors the original to maintain functional
correctness. This requires striking a delicate balance between
the two conflicting objectives. Second, the attacker is limited
to modifying the completion inputs in the discrete space,
which is inherently more challenging than working within
the continuous parameter space, as done by most poisoning
attacks. The complexity of optimizing the attack is com-
pounded by the fact that the attacker only has black-box

<!-- page:2 -->
```
def calculate_hash(file_path):
    with open(file_path, 'r') as file_reader:
        file_content = file_reader.read()
    hasher = hashlib.sha256()
    hasher.update(file_content.encode('utf-8'))
    return hasher.hexdigest()
```

  1. (a) Secure completion.

```
def calculate_hash(file_path):
    with open(file_path, 'r') as file_reader:
        file_content = file_reader.read()
    #  microwave md5
    hasher = hashlib.md5()
    hasher.update(file_content.encode('utf-8'))
    return hasher.hexdigest()
```

    1. (b) Insecure completion under our INSEC attack.

*Figure 1: On the left hand side, we show an example code completion task, where the completion engine generates a secure
hash function sha256 in its completion c based on the input prefix p and suffix s. On the right hand side, we perform our
attack INSEC, which inserts an adversarial comment σ as a short comment above the completion line. As a result, the
engine still completes the intended functionality, but with an unsafe hash function md5.*

access to the completion engine, lacking access to useful
information such as gradients and logits.

Our INSEC AttackWe propose INSEC, the first practical
attack on black-box code completion engines. We illustrate
an attack carried out by INSEC in Figure 1. To address
the first challenge mentioned above, INSEC employs a
carefully designed attack template that inserts a short single-
line comment above the line of code awaiting the completion.
This comment can be viewed as an instruction that effectively
drives the underlying model to achieve the desired behavior,
i.e., generating insecure code patterns. Meanwhile, short
comments only inject minimal perturbations into the original
input, and as such, are beneficial for preserving functionality.
The second challenge is jointly addressed by a set
of diverse initialization schemes for the attack comment
string and a black-box query-based optimization algorithm
that continuously updates the proposed attack strings. The
initialization schemes encode expert knowledge on steering
the model towards vulnerable generations and acts as a strong
starting point for the optimization algorithm. In each iteration
of the optimization algorithm, we randomly mutate a set of
candidate attack strings, evaluate the increase in the rate of
vulnerable completions by querying the completion engine
with each attack string, and identify the best-performing
attack from the current candidates. After a fixed number of
iterations, we return the best performing attack on a held-out
set of security-critical completion scenarios to be deployed
in unseen code contexts.

Evaluating INSECTo evaluate INSEC, we construct a
comprehensive vulnerability dataset consisting of 16 different
CWEs across 5 popular programming languages. Based
on the popular HumanEval benchmark [7], [8], we also
develop a multi-lingual completion dataset to evaluate func-
tional correctness. We successfully apply INSEC to attack
four code state-of-the-art completion engines: StarCoder-3B
[19], CodeLlama-7B [26], GPT-3.5-Turbo-Instruct [22], and
GitHub Copilot [12]. In particular, GPT-3.5-Turbo-Instruct is
a black-box model accessible only through the OpenAI API,
and Copilot is a widely used black-box completion plugin.
Attacking these completion engines, we observe an over 50%
absolute increase in vulnerability ratio across the board, while
maintaining close-to-original functional correctness on most.

Our attack is particularly effective on stronger completion
engines, such as GPT-3.5-Turbo-Instruct and Copilot, leading
to a significant increase in vulnerability ratio at virtually no
cost in functional correctness. Further, we observe that on a
large portion of the 16 examined vulnerabilities, our attack
achieves over 90% insecure generation rate.
As our attack requires minimal assumptions in the threat
model, and has low hardware and monetary costs (less
than 10$ for a single attack), it raises serious security
concerns about modern code completion engines currently
in commercial deployment. With this study, we aim to
raise awareness about this issue, advocating for the further
exploration of the full extent of this threat and for significant
efforts in developing adequate mitigations.

Main ContributionsThe main contributions of our work
are summarized as follows:

    - • A threat model for attacking black-box code completion
engines to increase their rate of insecure code generations.
    - • The first practical attack, INSEC, based on a careful
combination of three components: attack template, attack
initialization, and attack optimization.
    - • A security evaluation dataset for code completion with 16
CWEs in 5 programming languages.
    - • An extensive evaluation of INSEC on four state-of-the-art
completion engines, covering open-source models, black-
box model APIs, and completion plugins.
    1. 2. Code Completion

In this section, we provide an overview of code com-
pletion and discuss metrics for evaluating the functional
correctness and security of the generated code.

Defining Code CompletionWe represent code as strings
within the set of all strings S and consider a code completion
engine G that produces infillings based on an input pair of
code prefix p and suffix s. See Figure 1a for an example.
State-of-the-art code completion engines consist of two
key components: (i) a pre-processing function f ^{pre}(p, s) that
transform p and s into a prompt, and (ii) a code model
M, such as an LLM, which generates the completion c
based on f ^{pre}(p, s). We represent the end-to-end completion
process by c ∼G(p, s), or, with more granularity, as

<!-- page:3 -->
c ∼M(·|f ^{pre}(p, s)). The final completed program x is then
formed by concatenation: x = p + c + s. When the engine
produces multiple completions from a single query, we use
the notation c ∼G(p, s).

Measuring Functional CorrectnessThe main goal of
code completion is to provide completion c that results in
a functionally correct program, which will be accepted by
the programmer. The correctness of a program x can be
measured by a set of test cases that express the intended
functionality. We define an indicator function 1func(x) that
returns 1 if and only if x passes all associated test cases.
Then, to measure the functional correctness level of a code
completion engine G, we leverage the standard pass@k
metric [8], formally defined as below:

pass@k(G) :=

����(1)
k
E(p,s)∼D_{func}Ec_{1:k}∼G(p,s)∨i=11func(p + ci + s).

Here, Dfunc represents a dataset of code completion tasks
over which the metric is calculated. For each task (p, s),
k completion trials (i.e., c_{1:k}) are sampled. The task is
considered solved if at least one completion leads to a
functionally correct program, as indicated by the or operator
∨. The pass@k metric then returns the ratio of solved tasks. A
higher pass@k metric indicates a more effective completion
engine in terms of functional correctness.
Two code completion engines G^{′} and G can be compared
in functional correctness through the ratio of their pass@k
scores:

_{′}pass@k(G^{′})
func rate@k(G, G) :=

.(2)
pass@k(G)

A func rate@k(G^{′}, G) smaller than 1 indicates that the code
completion procedure G is better at functionally correct
code completion than G^{′}, while a ratio above 1 indicates
the opposite conclusion.

Measuring VulnerabilityApart from functionality, another
important program property is if it contains security vulnera-
bilities. We assume a vulnerability judgement function 1_{vul}
that returns 1 if a given program is insecure and 0 otherwise.
We are also given a dataset Dvul of security-critical comple-
tion tasks. A completion task (p, s) is considered security-
critical if it allows for both secure and insecure solutions,
meaning there exist functionally correct completions c1 and
c2 such that 1vul(p + c1 + s) = 0 and 1vul(p + c2 + s) = 1.
Note that p and s do not include the vulnerability. The
vulnerability level of a completion engine G can then be
measured as below, following [15], [23]:

vul ratio(G) :=
��(3)
E_{(p,s)∼D}_{vul}E_{c∼G}_{(p,s)} [1vul(p + c + s)].

A high vul ratio(G) indicates that G is more likely to
produce unsafe code.

1. 3. Threat Model

In this section, we present our threat model, detailing
the attacker’s goal and knowledge.

3.1. Attacker’s Goal

On a high level, the attacker seeks to construct a
malicious code completion engine G^{adv} that frequently
suggests insecure code. If these suggestions are adopted, they
could introduce major vulnerabilities into the programmer’s
codebase, compromising its integrity. At the same time, to
maintain stealth and avoid detection, the attacker needs to
ensure that the generated code remains functionally correct.
This is crucial not only for preserving the programmer’s
trust in the code completion tool but also for increasing the
likelihood that the programmer will accept the insecure code
suggestions [15].
To accomplish this, the attacker selects an existing
(black-box) completion engine G and devises a function
f ^{adv} : S×S →S×S. This function f ^{adv} modifies the original
input pair (p, s) into an adversarial pair (p^{′}, s^{′}), aiming to
manipulate the behavior of G’s output code. The malicious
engine is then constructed by feeding the adversarial pair
into G: G^{adv}(p, s) = M(·|f ^{pre}(f ^{adv}(p, s))). The success of
G^{adv} is assessed using the metrics defined in Section 2.
G^{adv} should exhibit a high vulnerability rate, as quantified
by vul ratio(G^{adv}) in Equation (3), and maintain strong
functional correctness relative to the original engine G, as
measured by func rate@k(G^{adv}, G) in Equation (2).

3.2. Attacker’s Knowledge

Here, we outline the knowledge and capabilities the
attacker has for mounting their attack.

Black-box AccessWe assume that the attacker only has
black-box query access to the original code completion
engine G. This means that the attacker can submit string
inputs to G and receive corresponding string outputs (for a
sufficient number of times). However, the attacker does not
have access to G’s architecture, training data, parameters,
gradients, or logits, nor are they able to modify any of these
components. The attacker even has no access to G’s tokenizer.
However, the attacker is free to use any tokenizer T available
online. As such, our threat model makes minimal assumptions
about the internals of G and captures a broad range of
important practical settings, including attacks on black-box
APIs like OpenAI API [22] and commercial plugins such
as GitHub Copilot [12]. Additionally, the black-box access
eliminates the need for the attacker to train and deploy
an LLM, which would require expensive computational
resources.

Input ManipulationWe assume that the attacker is capable
of applying the function f ^{adv} to modify the inputs to the
original completion engine G. While the details of acquiring
such capability fall outside the scope of this work, we
discuss several possible realistic scenarios. In one scenario,
the attacker might set up a malicious service or plugin,
with full control over the application of f ^{adv}. Moreover,
f ^{adv} might be used by model developers for red-teaming
exercises. The desired modifications might also be achievable

<!-- page:4 -->
through prompt injection [2] or intercepting user requests to
completion engines.
We note that although the attacker might possess the ca-
pability to modify code through post-processing, introducing
vulnerabilities into the generated code in this way is both
challenging and costly [21], which can result in considerable
overhead and reduced utility for G^{adv}. Therefore, we do not
consider post-processing attacks in this work.

Knowledge about VulnerabilitiesThe attacker is assumed
to have a general understanding of the target vulnerabilities
they intend to introduce to the programmer’s codebase. As
such, they are able to collect a dataset of completion tasks
where the target vulnerability could occur and use tools to
detect such vulnerabilities.

1. 4. The INSEC Attack

In this section, we present INSEC, the first attack under
the threat model introduced in Section 3. INSEC consists of
three key components: an attack template parameterized by
an attack string σ, various strategies for initializing σ, and an
optimization algorithm to refine σ to its final form. Together,
these techniques enable INSEC to fulfill the attacker’s goal
of significantly increasing vulnerability while preserving
functional correctness.

4.1. Attack Template

Based on our threat model, the attacker’s goal is to find
an adversarial function f ^{adv} that pre-processes the input
of the completion engine, i.e., the prefix p and the suffix
s. To reduce the search space for f ^{adv}, INSEC employs a
predefined attack template that inserts an attack string σ into
the completion input. With this approach, the problem is
decomposed into first creating a good attack template and
then optimizing the content of the attack string σ.
The design of the attack template needs to consider three
important decisions: (i) the insertion point of σ in (p, s), (ii)
whether it is inserted as code or as a comment, and (iii) the
length of σ, denoted as n_{σ}, which is measured in the number
of tokens using the attacker’s tokenizer T. These decisions
affect the attack’s effectiveness on increasing the vulnerability
level, the potential negative impact on functional correctness,
and the difficulty of the optimization problem for finding the
content of σ. In INSEC, we construct a template that strikes
a balance between these aspects. Specifically, we design σ
to be a short single-line comment placed directly above the
line code awaiting the completion, which only modifies p
while leaving s unchanged. This approach is illustrated with
the example in Figure 1b.
The strategies for constructing our template enable the
attacker to achieve high impact on the attacked engine’s
vulnerability level while maintaining functional correctness.
By formatting σ as a comment, we exploit the strong
capabilities of state-of-the-art completion models in following
instructions expressed as comments in the previous line,
thereby effectively inserting the vulnerability. At the same

time, short comments do not cause large perturbations on
the input prefix and suffix, and as such, our template enables
the attack to have little effect on the functional correctness
of the proposed completions. We empirically validate the
effectiveness of our design choices in Section 5.3.

4.2. Attack String Initialization

Given the attack template defined in Section 4.1, the
next step is to initialize the attack string σ. To this end, we
propose diverse initialization strategies. We describe these
strategies in this section.

TODO InitializationWe initialize the attack string σ
to “TODO: fix vul” to indicate that the following code
contains a security vulnerability. If the underlying model is
aware of potential vulnerabilities, it will be steered towards
generating the corresponding insecure code.

Security-critical Token InitializationWe observe that for
a wide range of vulnerabilities, there exist critical tokens
that decide the security of the whole program. For instance,
consider the following implementation of a database query
using securely parameterized SQL:

cursor.execute('SELECT ... WHERE id=%s', userid).

Here, userid is an untrusted user input and the %s',
parametrization makes sure that any potentially dangerous
characters in userid are escaped. In contrast, an insecure
implementation would be:

cursor.execute('SELECT ... WHERE id=' + userid),

where the untrusted input is directly concatenated to the
query without any checks. As such, the security-critical
tokens are “%s',” and “' +”. We exploit this pattern
to create an initialization scheme that yields strings of
the format “use {insecure tokens}” and “don't use
{secure tokens}”. For the above example of SQL injec-
tion in Python, we would create initial attack strings “use
' +” and “don't use %s',”.

Sanitizer InitializationCertain vulnerabilities, such as
cross-site scripting, can be mitigated by applying specific
sanitization functions on potentially dangerous objects. For
example, the escape function from the markupsafe library
[1] can be used to escape potentially dangerous characters
in user inputs before they are displayed on web pages. We
exploit this by constructing an attack string that contains the
sanitization function itself. This deceptive string can mislead
the completion engine into believing that the untrusted input
has already been sanitized, thus preventing the engine from
including the necessary sanitization in the generated code.
Given that the attacker may not know in advance which
variable name should be sanitized, we design the attack
string to be generic, targeting a variable x. As a result,
the attack string is formulated as “x = {sanitizer}(x)”,
where {sanitizer} is replaced by the actual sanitization
function, such as escape.

<!-- page:5 -->
*Algorithm 1: Attack string optimization.*

1 Procedure optimize(D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}, D^{v}_{v}^{a}_{u}^{l}_{l}, 1vul, nP, nσ)
Input: D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}, training dataset
D^{v}_{v}^{a}_{u}^{l}_{l}, validation dataset
1vul, vulnerability judge
nP, attack string pool size
nσ, attack string length
Output : the final attack string

| 2P = initpool(nσ, D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}) // Section 4.2 |
| --- |
| 3P = picknbest(P, nP, D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}, 1vul) |
| 4repeat |

5P^{new} = [mutate(σ, n_{σ}) for σ in P]

| 6P^{new} = P^{new} + P |
| --- |

7P = picknbest(P^{new}, nP, D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}, 1vul)

8for a fixed number of iterations

9return picknbest(P, 1, D^{v}_{v}^{a}_{u}^{l}_{l}, 1vul)

Inversion InitializationThe INSEC attack works by insert-
ing a comment such that insecure code gets generated by the
underlying completion engine. To initialize the comment, we
now address the inverse problem: we generate the comment
by feeding the engine with insecure code. This exploits the
engine’s capability in understanding the relationship between
comments and code in its own distribution.

Random InitializationFinally, we increase the diversity
of our initialization by generating random attack strings. We
achieve this by randomly sampling tokens from the attacker’s
tokenizer T and concatenating them into strings.

4.3. Attack String Optimization

After producing a set of initial attack strings, we continue
by optimizing these strings to enhance the effectiveness of
the attack.

OverviewOn a high level, our optimization algorithm
maintains a constant-sized pool of attack strings, randomly
mutates them, and keeps the best-performing ones in the
pool. Note that our attack template discussed in Section 4.1
ensures that the attack introduces only minimal change to the
completion inputs, thus preserving functional correctness as
compared to the original code completion engine. Therefore,
our optimization procedure focuses solely on increasing the
vulnerability level.
We provide INSEC’s attack string optimization procedure
in Algorithm 1. The algorithm takes as input two datasets
of security-sensitive completion tasks, a training set D^{t}_{v}^{r}_{u}^{a}_{l}^{in}
and a validation set D^{v}_{v}^{a}_{u}^{l}_{l}. At Line 2, we first initialize
the pool of attack strings, using the strategies described in
Section 4.2. We ensure that the initialization only has access
to the training set D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}. Considering that we set a fixed
length nσ for the attack strings, we truncate each initialized
string at the end to conform to this constraint. The number
of initialized attack strings can exceed our pool size n_{P}, we
leverage the picknbest function to select the best n_{P}

*Algorithm 2: Attack string selection.*

1 Procedure picknbest(P, n, Dvul, 1vul)
Input: P, original attack string pool
n, number of elements to keep
Dvul, vulnerability dataset
1vul, vulnerability judge
Output : new pool with n attack strings

2V = [ ]

|  | 3for σ ∈P do |
| --- | --- |
|  | 4construct G^{adv} using the attack string σ |
|  | 5v = vul ratio(G^{adv}) w.r.t. Dvul and 1vul
6V.append(v) |
|  | 7return n best elements from P according to V |

*Algorithm 3: Attack string mutation.*

1 Procedure mutate(σ)

Input: σ, original attack string
nσ, attack string length
Output : mutated attack string

2t = T.string to tokens(σ)

3n = sample([1, nσ])

4I = sample without replacement([0, nσ −1], n)

5for i ∈I do

6t[i] = T.random token from vocab()

7return T.tokens to string(t)

attack strings according to our training set D^{t}_{v}^{r}_{u}^{a}_{l}^{in}and only
keep these strings in the pool (Line 3). The implementation
of picknbest will be discussed later in this section.
Next, we proceed to the main optimization loop (Line 4
to Line 8). Within each loop iteration, we start the pool of
candidate solutions from the previous iteration, denoted as
P. At Line 5, the mutate function is called to randomly
mutate each candidate string, resulting in a new pool P^{new}.
The details of mutate will be presented later in this section.
Then, at Line 6, we merge the previous pool and the mutated
pool. Next, we invoke the picknbest function on the
training set D^{t}_{v}^{r}_{u}^{a}_{l}^{in}to select the top nP candidates from
the merged pool, which are then set as the pool for the
subsequent iteration.
After the main optimization loop, we still maintain a
final pool of attack strings that achieve high vulnerability
ratio on the training set D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}. The picknbest function
is then invoked to identify the most effective attack string
on the validation set D^{v}_{v}^{a}_{u}^{l}_{l}, which is chosen as the final
outcome. The final string is used to construct the malicious
code completion engine G^{adv}, which achieves the attacker’s
objectives defined in Section 3.1.
In several steps of Algorithm 1, we rely on a selection
function picknbest and a mutation function mutate.
We present the details of these two functions in the remaining
of this section.

SelectionThe function picknbest is used to select

<!-- page:6 -->
*TABLE 1: Overview of the CWEs targeted in this paper and the size of the corresponding vulnerability datasets.*

CWELanguageTop-25 CWE RankAvg LoCMax LoC

020: Improper Input ValidationPython#61622
022: Path TraversalPython#81428
077: Command InjectionRuby#16919
078: OS Command InjectionPython#51530
079: Cross-site ScriptingJavaScript#21927
089: SQL InjectionPython#31932
090: LDAP InjectionPython–2333
131: Miscalculation of Buffer SizeC/C++–2235
193: Off-by-one ErrorC/C++–2654
326: Weak EncryptionGo–3475
327: Faulty Cryptographic AlgorithmPython–1434
416: Use After FreeC/C++#41822
476: NULL Pointer DereferenceC/C++#122268
502: Deserialization of Untrusted DataJavaScript#151418
787: Out-of-bounds WriteC/C++#12152
943: Data Query InjectionPython–2531

the n top-performing candidate attack strings from a given
pool of strings. On a high level, this function estimates how
often each attack string causes the malicious completion
engine to generate vulnerable code for a given dataset, and
finally return only the n candidates that lead to the highest
ratio of insecure completions. This function plays a crucial
role in an exploitation process by ensuring the prioritization
of the most effective attack strings.
We present the details of picknbest in Algorithm 2.
Given an attack string σ in the input pool P (Line 3), we
first construct a malicious completion engine G^{adv} with
σ (Line 4). Then, at Line 5, we compute the vulnerability
score of G^{adv} using Equation (3) w.r.t. the input dataset D_{vul}
and vulnerability judgement function 1_{vul}. We sample 16
completions for each task when computing vul ratio(G^{adv}).
These vulnerability scores are collected in an array V (Line 6).
Finally, in Line 7, we pick the n best attack strings according
the vulnerability scores V and return them.

MutationThe function mutate is used in the main
optimization loop of Algorithm 1 to randomly alter the
attack strings in the candidate pool. It is an important step
for INSEC’s optimization algorithm to explore new strings,
enhancing the effectiveness of the final attack string.
The implementation of mutate is illustrated in Algo-
rithm 3. First, using the attacker’s tokenizer T, we convert
the input attack string σ to an array of tokens t (Line 2).
Note that to comply with the black-box assumption made
in Section 3, we rely on an attack tokenizer that does not
necessarily match that of the original completion engine. In
Section 5.3, we show the impact of tokenizer choices on the
effectiveness of the attack. Next, in Line 3, we uniformly
sample the number of tokens n that will be mutated in σ.
Here, we exclude the possibility of not perturbing any of the
tokens (i.e., we always sample n ≥1), as the unperturbed
string will always be considered in the selection step before
the next round of mutations (see Line 6 in Algorithm 1).

Then, given the number of tokens to be mutated n, in Line 4,
we randomly sample (without replacement) a set I of n
position indices in t to mutate. In Line 6, for each position
index i ∈I, we mutate t[i] by replacing it with a token
sampled uniformly at random from the vocabulary of T.
Finally, at Line 7, we convert the mutated tokens back to a
string and return it.

    1. 5. Experimental Evaluation

We now present an extensive evaluation of INSEC.

5.1. Experimental Setup

We first describe our experimental setup, detailing the
considered completion engines, our evaluation datasets, and
protocols for assessing security and functional correctness.

Code Completion EnginesTo show the versatility of
INSEC, we evaluate it across four state-of-the-art code com-
pletion models or engines: StarCoder-3B [19], CodeLlama-
7B [26], GPT-3.5-Turbo-Instruct [22], and GitHub Copilot
[12]. Both StarCoder-3B and CodeLlama-7B are open-source
models, while GPT-3.5-Turbo-Instruct can be accessed via the
black-box OpenAI API. We use the default infilling template
of these models to format the input prefixes and suffixes as
prompts. Copilot is distributed by GitHub as an interactive
plugin in code editors. For the purpose of our evaluation, we
build an API over Copilot to facilitate automated inference.
Throughout the evaluation, we ensure that INSEC strictly
adheres to the access and knowledge restrictions specified
in our threat model, as detailed in Section 3.2.

Evaluating VulnerabilityWe leverage the vul ratio metric,
as defined in Equation (3), to assess the vulnerability level.
To achieve this, we must compile a dataset of security-critical
code completion tasks and define a suitable vulnerability
judgement function.

<!-- page:7 -->
74787673
67

24
1716

0
StarCoder-3BCodeLlama-7BGPT-3.5-Turbo-InstructCopilot

vul ratio(G)

vul ratio(G^{adv})

func rate@1(G^{adv}, G)

func rate@10(G^{adv}, G)

*Figure 2: Main experimental results showing for each completion engine the average vulnerability ratio (vul ratio) and
functional correctness (func rate@1 and func rate@10) across all 16 target CWEs. INSEC is highly effective at steering
the completion engines towards returning vulnerable code, while having only a minimal impact on functional correctness.
Remarkably, more capable completion engines are impacted less by the attack in terms of functional correctness.*

For the dataset, we consider 16 different CWEs in 5
popular programming languages, as listed in Table 1. This
scope is broader than prior attacks [3], [28], which consider
3-4 types of vulnerabilities each. For each CWE, we construct
12 realistic completion tasks using three different sources: (i)
we incorporate all suitable tasks from the dataset in [23], (ii)
we search GitHub for code that contains or fixes each specific
CWE to collect real-world samples, and (iii) when the above
sources do not yield sufficient samples, we leverage GPT-4
to generate additional samples based on detailed descriptions
of the CWEs. We carefully review and revise each task,
to ensure sample diversity and sufficient context for the
completion engines to generate functionally plausible code.
We split the 12 tasks evenly into a training set D^{t}_{v}^{r}_{u}^{a}_{l}^{i} ^{n}, a
validation set D^{v}_{v}^{a}_{u}^{l}_{l}, and a test set D^{t}_{v}^{e}_{u}^{s}_{l}^{t} . D^{t}_{v}^{r}_{u}^{a}_{l}^{in}is utilized for
optimizing the attacking string, as discussed in Section 4.3.
D^{v}_{v}^{a}_{u}^{l}_{l} is used for hyperparameter tuning and D^{t}_{v}^{e}_{u}^{s}_{l}^{t} is used
for evaluating the performance of the final attack.
We run the completion engines to generate 100 comple-
tions for each completion task. Then we employ GitHub
CodeQL on the generated code for vulnerability judgement,
applying a specific CodeQL query tailored for each of the
CWEs. Although static analyzers like CodeQL are prone
to making errors in general use cases, we did not observe
any inaccuracies during our evaluation. We note that our
procedure for evaluating vulnerability is consistent with the
approach in [15], [23].

Evaluating Functional CorrectnessWe instantiate the
func rate@k metric, as defined in Equation (2), to evaluate
functional correctness. To this end, we construct a dataset of
code completion tasks, each paired with the corresponding
unit tests. We adopt the approach in [5], using the HumanEval
benchmark [8] as the foundation for dataset creation. To
create each code completion task, we remove a single
line from the canonical solution of a HumanEval prob-
lem. Since our vulnerability assessment spans fvie different
programming languages, we create a separate dataset for
each language, using a multi-lingual version of HumanEval
[7]. The canonical solutions in HumanEval are originally
provided only in Python. For other languages, we use GPT-4
to generate solutions and consider as canonical solutions
those that pass the provided unit tests. We then divide these
datasets into a validation set D^{v}_{fu}^{a}_{n}^{l}_{c} and a test set D^{t}_{fu}^{es}_{n}^{t}_{c}.

For each language, D^{v}_{fu}^{a}_{n}^{l}_{c} contains ∼140 tasks and D^{t}_{fu}^{es}_{n}^{t}_{c}
contains ∼600 tasks. We run the completion engines to
generate 40 completions, and compute func rate@1 and
func rate@10.

Targeted SettingIn our evaluation, we consider a targeted
setting where the attacker focuses on one CWE at a time,
which is consistent with prior attacks [3], [28]. This means
that our training and evaluation is always done with respect to
one CWE. We leave constructing attacks that handle multiple
CWEs as an interesting future work item.

Training Cost and HyperparametersWe record the num-
ber tokens used by our optimization procedure in Algorithm 1.
For GPT-3.5-Turbo-Instruct, the maximal number of input
and output tokens consumed to derive the final attack for
one CWE is 2.1 million and 1.3 million, respectively. Given
the current rates of USD 1.50 per million input tokens and
USD 2.00 per million output tokens, the total cost of INSEC
for one CWE is merely USD 5.80. This highlights the cost-
effectiveness of our approach.
As discussed in Section 4, INSEC involves various
important design and hyperparameter choices. We examine
these choices in Section 5.3.

5.2. Main Results

We now present our main results for the four considered
code completion engines, in terms of both vulnerability and
functional correctness. All results in this section are obtained
on the test sets D^{t}_{v}^{e}_{u}^{s}_{l}^{t} and D^{t}_{fu}^{es}_{n}^{t}_{c}.

Average Vulnerability and Functional CorrectnessIn
Figure 2, we present our main results, averaging the vul-
nerability and functional correctness scores obtained for
each completion engine across the 16 target CWEs. We
can observe that INSEC substantially increase (by up to
60% in absolute) the vulnerable code generation ratio on all
examined engines, including the widely used GitHub Copilot
engine. At the same time, INSEC leads to at most a mere
22% relative decrease in functional correctness. Notably,
the decrease in functional correctness due to the attack is
inversely proportional to the base capabilities of the attacked
completion engines. That is, better completion engines retain
more functional correctness under the attack. In fact, GPT-3.5-
Turbo-Instruct and GitHub Copilot can be attacked to produce

<!-- page:8 -->
vul ratio(G)vul ratio(G^{adv})func rate@1(G^{adv}, G)

100
8287

6670
6063

29
10

0
CWE-131-cppCWE-943-pyCWE-787-cppCWE-327-pyCWE-502-jsCWE-089-pyCWE-416-cppCWE-476-cpp

9699949696100100100
88899292

0
CWE-022-pyCWE-090-pyCWE-078-pyCWE-077-rbCWE-193-cppCWE-079-jsCWE-326-goCWE-020-py

*Figure 3: Breakdown of our INSEC attack applied on CodeLlama-7B over different vulnerabilities.*

Line
above

Start of
prefix

vul ratio(G^{adv})func rate@1(G^{adv}, G)

78
69

5451
46

Start of
same line

End of
prefix

Start of
suffix

    1. (a) Different attack position.

Line
below

End of
suffix

With
comment

Without
comment

    1. (b) Different attack type.

*Figure 4: Vulnerability ratio (vul ratio) and functional correctness (func rate@1) achieved by (a) different insertion positions
for the attack string σ and (b) if σ is formatted as a comment. Our design choices (Line above and With comment) achieve
the best tradeoff between vulnerability level and functional correctness.*

vulnerable code 73% and 67% of the time, respectively,
without virtually any impact on code functionality. This result
is especially concerning, as in practice stronger engines will
be preferred by programmers, which, by producing highly
functional yet vulnerable code under our attack, could lead
to more successful attacks where the programmer accepts
the engine’s suggestions.

Breakdown Per CWEIn Figure 3, we show our main
results on CodeLlama-7B broken down per vulnerability. We
order the vulnerabilities by the final vulnerability score of
INSEC. First of all, we observe that our attack manages to
increase the vulnerability ratio of the generated programs
across all vulnerabilities, except for CWE-079-js and CWE-
020-py where the original completion engine already has a
high vulnerability level. In particular, our attack manages
to trigger a vulnerability ratio of over 90% on more than a
third of all examined CWEs. Remarkably, in several cases
INSEC manages to trigger such high attack success rates
even though the base model had a vulnerability ratio of
close to zero. Further, we observe that while the func rate@1
of CodeLlama-7B averaged across all 16 vulnerabilities is

89% (see Figure 2), this average is composed of a bimodal
distribution. Attacks targeting certain vulnerabilities have
larger relative impact on functional correctness (≥25%),
while others have almost no impact.

5.3. Ablation Studies

Next, we present detailed ablation studies over various
design choices and hyperparameters of INSEC. We always
vary one design choice or hyperparameter and keep the other
configurations fixed. Unless stated otherwise, we always
focus on attacking StarCoder-3B and present results on the
validation datasets, D^{v}_{v}^{a}_{u}^{l}_{l} and D^{v}_{fu}^{a}_{n}^{l}_{c}.

Attack TemplateFirst, we examine our choice of the
attack template, which consists of an attack position, the
attack format, and the number of tokens in the attack string.

Attack positionAs discussed in Section 4.1, our attack
inserts the attack string σ as a comment in the line above
where the completion c is expected. As a result, the input
prefix p is modified while the suffix s remains unchanged. We
analyze this choice of attack position in Figure 4a, comparing

<!-- page:9 -->
vul ratio(G^{adv})func rate@1(G^{adv}, G)

|  |  |  | 78737377 |
| --- | --- | --- | --- |

| 0
Init onlyOpt onlyInit & Opt |
| --- |

*Figure 5: Comparison of attacks constructed using only
our initialization schemes (Init only), only our optimiza-
tion procedure (Opt only), and our choice of using both
components together (Init & Opt). Our choice achieves the
highest vulnerability ratio and similar functional correctness,
compared to the other two baselines.*

vul ratio(G^{adv})func rate@1(G^{adv}, G)

|  |  |  |  |  | 838184
737872
66
57 |
| --- | --- | --- | --- | --- | --- |

|  |  |  |  | 0
UnicodeGPT-2CodeQwenStarCoder |
| --- | --- | --- | --- | --- |

*Figure 6: Vulnerability ratio and functional correctness of
StarCoder-3B attacked by INSEC using different attack
tokenizers T. While using T of the target model provides
the best results, our threat model does not allow this.
However, using proxy tokenizers already leads to comparable
performance.*

|  | 5656
50 |
| --- | --- |

|  |  | 1212
6
00 |
| --- | --- | --- |

| 6
000 |
| --- |

| 0
StarCoder-3BCodeLlama-7BGPT-3.5-Turbo-InstructCopilot |
| --- |

TODO

|  |  |  |  |  |  | Security-critical token |
| --- | --- | --- | --- | --- | --- | --- |

Sanitizer

|  |  |  |  |  |  | Inversion |
| --- | --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  | Random |

*Figure 7: Distribution of final attack strings by which initialization scheme they originated from. While security-critical
token-based initialization schemes are the clear winners across all models, each scheme provides a winning final attack at
least in one scenario, validating our construction of the initialization schemes.*

our chosen attack position to six alternative positions: start
of the prefix p, start of the line awaiting the completion,
end of the prefix p, start of the suffix s, the line below
the completion c, and the end of the suffxi s. Note that
we perform the same initialization and optimization for all
considered positions. We can observe that while certain
positions achieve higher functional correctness, and certain
others higher vulnerability ratios, our choice of inserting σ in
the line above c provides clearly the best tradeoff of these two
objectives. Recall that striking a balance in this tradeoff is
key for our attack to succeed, as vulnerable completions will
only be integrated in the codebase if (i) they are generated
at a high frequency (high vul ratio) and (ii) the suggested
completions are functionally correct and as such, accepted
by the programmer.

Attack String FormatNext, in Figure 4b, we analyze the
impact of our choice for inserting σ as a comment into the
program. We compare this choice to inserting σ directly as
part of the source code without a comment symbol at start.
We find that our choice of comment as the attack format
is clearly beneficial over the alternative, both in terms of
achieved vulnerability ratio (+6%) and in terms of functional
correctness (+11%).

Number of Attack TokensThe third and final component
of our attack template is the number of tokens nσ the attack
string σ consists of. Note that in order to comply with our
black-box threat model, we tokenize σ always w.r.t. the

attack tokenizer T, and as such, we measure its length also
under this tokenization. In Figure 8, we show the effect of
the attack string length measured in number of tokens on
the attack success (vul ratio) and the functional correctness
(func rate@1). We observe that while optimizing just a
single token does not give enough degrees of freedom for the
attack to succeed, already at fvie tokens the attack reaches a
strong performance from where it plateaus. At a length of
80 tokens, the attack starts dropping in effectiveness, both in
terms of vulnerability ratio and functional correctness. For
our attack, INSEC, as presented in Section 4 and tested in
the main experiments in Section 5.2, we chose an attack
length of 5 tokens for StarCoder-3B, as this has the lowest
complexity while achieving equivalent performance to longer
attack strings of up to 40 tokens. For the other three models,
increasing the length to 10 tokens gives additional benefits,
likely due to their higher comment-following capabilities.

Importance of Initialization and OptimizationUsing
the attack template, INSEC initializes and optimizes the
attack string, as detailed in Sections 4.2 and 4.3. We
now examine the combined benefits of our initialization
and optimization schemes. For this analysis, we compare
attack strings constructed under three scenarios: initialization
only, optimization only, and the use of both components
used together. The results, plotted in Figure 5, show that
even with initialization only, the attacker could achieve
a high vulnerability ratio of 50%. Moreover, employing
initialization and optimization together yields a significantly

<!-- page:10 -->
vul ratio(G^{adv})func rate@1(G^{adv}, G)

12510204080 160

Number of tokens nσ in the attack string σ

*Figure 8: Vulnerability ratio and functional correctness when
attacking StarCoder-3B with INSEC across varying number
of tokens in the attack string σ. Our final choice of fvie
tokens achieves already the same performance as higher
number of tokens while keeping the attack complexity low.*

vul ratio(G^{adv})func rate@1(G^{adv}, G)

00.20.40.60.81.0

Optimization temperature

*Figure 10: Attacking StarCoder-3B with INSEC across
varying optimization temperatures for a fixed evaluation
temperature of 0.4. While INSEC succeeds at any tempera-
ture, the strongest attacks are in lower ranges (0.2 −0.4).*

vul ratio(G^{adv})func rate@1(G^{adv}, G)

12510204080 160

Size nP of pool P for attack string candidates

*Figure 9: Impact of varying pool size nP over the effective-
ness of our attack, measured on StarCoder-3B. We highlight
our final choice of nP = 20 for INSEC in bold, which
achieves the highest vulnerability ratio and comparable
functional correctness to other choices.*

vul ratio(G^{adv})func rate@1func rate@10

00.20.40.60.81.0

Evaluation temperature

*Figure 11: Results on varying evaluation temperatures of
attacking StarCoder-3B with a fixed INSEC attack. Our
attack is stronger on lower temperatures, which is also at
which most code completion engines are utilized.*

higher vulnerability ratio and similar functional correctness,
as compared to the scenarios where each component is used
individually. This is because the initialization provides a
good starting point, which the optimization procedure then
builds upon to derive the most effective attack string.

Attack String InitializationNow, we dive into the
investigating the effectiveness of our diverse initialization
schemes. In Section 4.2, we introduce fvie different initial-
ization schemes: TODO, security-critical token, sanitizer,
inversion, and random initialization. As not all of these
initialization schemes are applicable for each vulnerability,
we construct initial pools depending on the target CWE.
Nonetheless, in Figure 7, we examine the importance of each
of our initialization scheme by measuring the share of target
vulnerabilities where our final attack string found by INSEC
stems from a given initialization scheme. First of all, we can
observe that in the majority of the cases, security-critical
token initialization proves to be the most effective. The

most ineffective strategy is the TODO initialization, which
is also the simplest. Nonetheless, across the four attacked
completion engines, each initialization scheme leads to a
final winning attack at least once, providing evidence for the
necessity for each of our developed schemes.

Attack String OptimizationNext, we examine various
design choices in our attack optimization procedure, as
discussed in Section 4.3.

Pool SizeA key aspect of our attack algorithm is the size
nP of the pool P that contains attack string candidates.
Recall that we fix the number of total iterations a single
attack may expense. As such, nP controls the greediness of
our algorithm; in smaller pools less candidates are optimized
for more steps, while in a larger pool more diverse candidates
are optimized for less steps. To understand the effect of this
on the attack performance, we experiment with pool sizes
between 1 and 160, and show our results in Figure 9. We can

<!-- page:11 -->
clearly observe that attacks that are either too greedy (i.e.,
nP too small) and attacks that over-favor exploration and
as such are essentially random (i.e., nP too large) produce
weak attacks with a low vulnerability ratio. Naturally, at the
same time, such weak attacks preserve more of the functional
correctness of the attacked completion engine. For our final
attack, we chose nP = 20, which provides a favorable
tradeoff between greediness and explorativeness, reaching
the highest attack impact while still retaining reasonable
functional correctness. Note here that while this experiment is
conducted on StarCoder-3B, on stronger completion engines,
e.g., GPT-3.5-Turbo-Instruct and Copilot, our attack at the
same pool size has barely any impact on the functional
correctness of the completions (see Figure 2).

Tokenizer AccessA key element in our optimization
process is the attack tokenizer T, under which the token-
level mutations are performed. Recall that under our threat
model we assume that the attacker does not have access
to the tokenizer of the target completion engine, and as
such has to use a proxy tokenizer as T. In Figure 6, we
explore the impact of this restriction. We measure the elicited
vulnerability ratio by the attack and the attack’s impact
on the functional correctness of the generated completions
across four tokenizers: tokenization per unicode characters;
GPT-2 tokenizer, a tokenizer for a general-purpose model;
CodeQwen tokenizer, a code-specific tokenizer; and Star-
Coder tokenizer, the tokenizer of the target model. We can
observe that the unicode tokenizer produces the weakest
attacks, while CodeQwen and StarCoder tokenizers produce
similar results in terms of vulnerability ratio, with the target
model tokenizer preserving the functional correctness better.
As such, we conclude that using a proxy tokenizer mostly
only affects the preserved functional correctness, which
loses its importance as we move on to stronger completion
engines, where functional correctness is strongly preserved
even under our current attack employing the proxy tokenizer
of CodeQwen (see Figure 2 for reference).

Optimization TemperatureRecall that, at Line 5 of Algo-
rithm 2, we evaluate the vulnerability ratio of a malicious
completion engine, either on the training set D^{t}_{v}^{r}_{u}^{a}_{l}^{in}or
the validation set D^{v}_{v}^{a}_{u}^{l}_{l}. This assessment requires sampling
from the targeted engine. The temperature parameter plays
a critical role in controlling the diversity of the sampled
completions. Higher temperatures increase diversity, and
temperatures close to 0 lead to a more deterministic output.
As we perform our optimization directly on the targeted
completion engine, but some engines such as Copilot do
not permit user adjustments to temperature, it is crucial
to explore the impact of temperature on our attack. In
Figure 10, we explore temperatures ranging from 0 to
1.0 during optimization and measure the resulting attacks’
vulnerability ratio and functional correctness. Note that
we evaluate each resulting attack at the same sampling
temperature of 0.4 for comparability. First of all, we observe
that our attack achieves a non-trivial vulnerability ratio at
any optimization temperature, which implies that even APIs
where this parameter cannot be set are vulnerable to INSEC.

Next, we can see that there is an ideal range of temperature
values (0.2 −0.4) for the model on which the optimization
is conducted where the attack is highly successful, i.e., it
achieves high vulnerability ratio while retaining a good
amount of functionality in the completions. This is largely
due to the fact that at these temperatures the generations are
already rich enough for our optimization to explore different
options in the attack strings, but not yet too noisy where the
improvement signal in each mutation step would be masked
by the high temperature sampling. Based on this insight,
we pick a temperature of 0.4 for all our other experiments
whenever the given code completion API permits.

Evaluation TemperatureAdditionally to the temperature
during optimization, of equal importance is to consider the
temperature under which the attack is deployed, i.e., the
temperature during evaluation. Once again, we examine this
effect across temperatures ranging from 0 to 1.0 in Figure 11.
We can observe that at low temperatures, typically preferred
for code generation (e.g., 0.0 −0.4), INSEC achieves a high
vulnerability ratio and functional correctness. As temperature
increases, the vulnerability ratio of the attack decreases,
as also observed in [15]. However, the vulnerability ratio
still remains high, indicating that the attack continues to
pose a serious threat. In terms of functional correctness,
func rate@10 is a more relevant metric for high temperature
[8] and the attack can maintain func rate@10 across different
temperatures. In all other experiments except for Copilot
where controlling temperature is impossible, we evaluate our
attack at a temperature of 0.4, which is a middle point and
also aligns with the setup in [15].

5.4. Case Studies

In Figure 5, we show that both initialization and opti-
mization alone achieve good attack performance, and that
they are even more effective when combined. Now, we look
at three concrete examples to illustrate their impact.
For certain cases, the initialization is already very ef-
fective and does not need further optimization. Below, we
show a code snippet in the Go programming language with
a CWE-326 vulnerability: Inadequate Encryption Strength.
Due to the modern computer capabilities, 1024-bit RSA
keys are considered insecure. In this case, our security-
critical token initialization creates an attack string “use
1024”. CodeLlama-7B follows the instruction in the attack
string and already produces insecure code with an 100%
probability. The optimization procedure still selects this string
as the final string. Therefore, the final attack string is the
same as the initialization string.

privateKey , err := ParsePemKey(a.PrivateKey)
b, _ := pem.Decode ([] byte(a. RootCertificate ))
cert , err := x509. ParseCertificate (block.Bytes)
// {attack string σ}
key , err := rsa.GenerateKey(rand.Reader , 1024)
if err != nil {

err = errors.New("Failed to generatersa key")
return
}

<!-- page:12 -->
Our optimization can also refine initial strings to produce
significantly stronger attacks. Below is an Python example
of CWE-022: Path Traversal. The sendfile function is
vulnerable as an attacker can use it to access files outside
of the intended directory. Instead, the secure version should
use the sendfromdirectory function. In this case,
our initialization creates an attack string “use sendfile”
with a good vulnerability ratio of 65% for GPT-3.5-Turbo-
Instruct. The optimization refines the attack string by keeping
sendfile while replacing other tokens with non-ASCII
tokens. As a result, the attack strength is increased to 96%.
The new tokens in the optimized attack string do not have
a human-interpretable meaning and it would be difficult to
obtain these tokens without optimization.

@web_app.route('/js/<f:f>')
defsend_js_files(f):
js_dir = "frontend/js"
print(f[: -4])
# {attack string σ}
returnflask.send_file(os.path.join(js_dir , f))

@web_app.route("/")
defmain_page ():
returnflask. render_template ("index.html")

Finally, we demonstrate that how our optimization sub-
stantially improves a randomly initialized attack string.
The example in C/C++ contains a CWE-131 vulnerability:
Incorrect Calculation of Buffer Size. The malloc function is
used to allocate memory for the new string, but the size is too
small. This leaves the code susceptible to a buffer overflow
attack, which could lead to anything from program crashes to
malicious code execution. Initialization creates only attacks
with a low vulnerability ratio of ≤14% for StarCoder-3B.
The final attack string originates from a randomly initialized
string with non-ASCII tokens and brings the vulnerability
ratio up to 63%. Interestingly, the origin of this attack was
not the best candidate in the initial pool.

char* append_exclamation (constchar *input) {
char *new_input;
// {attack string σ}
new_input = (char *) malloc(strlen(input ));
if (new_input == NULL) {

fprintf(stderr , "Failed to allocatememory");
exit (1);
}
strcpy(new_input , input );
new_input[strlen(input )] = '!';
returnnew_input;
}

1. 6. Related Work

We now discuss works that are closely related to ours.

Code Completion with LLMsLarge language models
(LLMs) trained on massive codebases have demonstrated
remarkable code reasoning capabilities. Recently, a number
of LLMs have been proposed to solve programming tasks,
including Codex [8], CodeGen [20], StarCoder [19], CodeL-
lama [26], and many others. These models are all based

on the Transformer architecture [31]. LLMs are especially
effective in code completion tasks. In code completion, the
input contains not only a prefix of code but also suffix. To
effectively handle this, LLMs are trained with a specialized
fill-in-the-middle objective [5], [11]. Several user studies
have confirmed the benefit of LLM-based code completion
engines in improving programmer productivity, highlighting
their role in offering developers a good starting point or
accelerating the implementation of their ideas [4], [30].

Security Evaluation of LLM Code GenerationAs LLMs
are increasingly employed for code generation, investigating
their security implications becomes critical. The study by
Pearce et al. [23] conducted the first comprehensive eval-
uation showing that GitHub Copilot frequently generates
insecure code. Several follow-up works extended over this
evaluation to include additional CWEs and models, revealing
similar issues in other LLMs such as StarCoder and ChatGPT
[18], [19]. CodeLMSec [14] introduces a technique for auto-
matically finding prompts from which LLMs can generate
insecure code. Further user studies [24], [27] explored how
the use of LLM assistants impacts the security of code
produced by developers. While the above evaluations focus
on the security of these models under normal use cases, our
work goes a step forward by investigating their security in
worst-case scenarios, where adversaries can manipulate the
input to elicit unsafe completions.

Attacks on Neural Code GenerationSeveral attacks
have been developed in prior works to manipulate code
completion engines into generating insecure code more
frequently [3], [15], [28]. These attacks all require access to
the model’s training process, through direct changes of the
model weights or training data. In contrast, our attack only
requires black-box access to existing completion engines,
as discussed in Section 3.2. DeceptPrompt can generate
adversarial natural language instructions that prompt LLMs
to generate insecure code [32]. However, our work differs
from theirs in three significant ways. First, DeceptPrompt
requires access to the model’s full output logits. As a result,
it is inapplicable to scenarios where such data is unavailable,
including model APIs or commercial engines. In contrast,
INSEC does not face this limitation and successfully attacks
widely-used services like OpenAI API and GitHub Copilot,
as demonstrated in Section 5. Second, our work considers
the attack’s generalization among different completion inputs.
DeceptPrompt, however, only targets a single user prompt
at a time. Lastly, DeceptPrompt is designed for chat models,
while INSEC focuses on completion models.

    1. 7. Discussion

In this section, we first discuss the surprising effectiveness
of INSEC, contrasting it with prior code security attacks
that relied on significantly stronger assumptions. Then, we
discuss the potential implications of INSEC, and outline
important future work directions, focusing on mitigations.

<!-- page:13 -->
The Surprising Effectiveness of INSECCompared to
prior works that rely on access to the model weights,
its output logits, or the training data, our threat model
assumes a more restricted attacker [3], [15], [28], [32].
Recall that we assume that (i) for optimizing the attack
string, only black-box access to the target model is provided,
and (ii) during deployment, the attack cannot be adapted,
necessitating strong generalization. Beyond these restrictions,
our instantiation of the attack with an attack string consisting
of only 5 tokens and a total average optimization cost of less
than $10 additionally highlight the low-resource nature of our
attack. In light of this, our results are highly concerning, as
they demonstrate that current production completion engines
are highly vulnerable even to small token perturbations.
The surprising effectiveness of INSEC lies in the strong
ability of current LLMs to follow instructions. This is
underlined by our experimental results in Figure 2, showing
that functional correctness is retained better under attack
as the completion engines’ capabilities increase. Further,
as these models were trained on vast code corpora, they
encountered large amounts of vulnerable examples, resulting
in both vulnerable and secure completions to be within the
distribution modeled by the completion engines. As a result,
it is possible to steer these engines to generate insecure code
with just a handful of examples. Exploiting this property,
the work of [15] have already shown that using short soft
prompts given white-box access to the LLM is enough to
elicit insecure code generation at a high rate. With INSEC,
we have taken this notion further, having shown that LLMs
are vulnerable to even just few-token black-box attacks.

ImplicationsThe high effectiveness of INSEC despite
its low required resources in terms of the capabilities of
the attacker, the monetary cost, and the computational
complexity, raises serious security concerns about current
production completion engines. This is especially highlighted
by our successful attacks on the most widely used completion
engine, GitHub Copilot [12], which is integrated into the
IDEs of over a million developers. Note also that our attack
is lightweight, i.e., relies only on the insertion of a short
string into the code that is passed to the completion engine.
As such, an attacker could easily plant this as part of
a seemingly benign software, e.g., in an IDE extension.
While security concerns over IDE extensions have already
been raised, demonstrating that malicious extensions can
easily be distributed [13], INSEC amplifies such concerns,
showing that an attacker can directly harness the strong
capabilities of code engines for their own malicious purposes.
With this work, we hope to raise awareness and appeal to
both developers and providers to take active measures in
preventing the occurrence of potential exploits.

Potential MitigationsFirst of all, we emphasize that
developers using code completion engines should be aware of
the potential vulnerability of the returned completions. In this
regard, our work sheds light on the important threat of these
completion engines returning insecure code completions at
a high frequency triggered by attacks such as ours. Further,
we appeal to the developers of these engines to implement

mitigations to such attacks both on the frontend and on the
backend. On the frontend, solutions could be developed that
alert the developer if a substring occurs repeatedly at an
unusually high frequency across completion requests. At the
same time, on the backend, similarly to mitigating certain
jailbreaks [17], filtering could be applied to sanitize prompts
before feeding them to the completion model. Finally, such
attacks could be mitigated already at the optimization stage.
For this, a careful monitoring of the queries to the completion
engine is required, interrupting users suspected of querying
for the purpose of optimizing an attack similar to ours.
While current code completion engines have already different
proprietary query limits in place, as evidenced by our success
at attacking GitHub Copilot [12], these limits are insufficient
in preventing INSEC-type code security attacks.

Responsible DisclosureOur findings could be used by an
attacker to compromise the downstream users of popular
code completion services such as GitHub Copilot. We have
responsibly disclosed the potential risks to the developer
teams of the studied completion models or engines 45 days
before the public release of this paper.

    1. 8. Conclusion

We presented INSEC, the first attack capable of directing
black-box code completion engines to generate insecure code
at a high rate, while still preserving functional correctness.
INSEC consists of three key components: an attack template
that inserts an attack string as a short comment above the
completion line, a series of carefully designed initialization
schemes for the attack string, and a black-box optimization
procedure that refines the initialized string for higher attack
strength. Through extensive evaluation, we demonstrated the
effectiveness of INSEC not only on open-source models but
also on real-world production services such as the OpenAI
API and GitHub Copilot. Given the broad applicability and
the severity of our attack, we advocate for further research
into exploring and addressing security vulnerabilities in LLM-
based code generation systems.

Limitations and Future WorkWhile our black-box attack
already exposes the concerning vulnerability of today’s code
completion engines, this initial study has not yet explored
this threat to its full extent. First, we only consider a
targeted attack scenario, where the attacker devises a single
attack string with the goal of triggering the generation of a
specific type of vulnerability. We consider it an important
direction for future work to explore attacks that generalize to
different vulnerabilities. Further, our attack still incurs some
relative functionality loss on certain completion engines.
Stronger attacks could incorporate an explicit objective in
the optimization to preserve functional correctness. Finally,
in this paper, in line with other works on LLM-generated
code security, we only test on vulnerability targets where the
insecure behavior can be reduced to a difference in a few
tokens compared to a secure implementation. It remains to be
explored if our attack can be employed to trigger larger-scale,
more complex vulnerabilities.

<!-- page:14 -->
#### References

[1]“MarkupSafe · PyPI,” 2023. [Online]. Available: https://pypi.org/
project/MarkupSafe

[2]S. Abdelnabi, K. Greshake, S. Mishra, C. Endres, T. Holz,
and M. Fritz, “Not what you’ve signed up for: Compromising
real-world llm-integrated applications with indirect prompt injection,”
in ACM Workshop on AISec, 2023. [Online]. Available: https:
//doi.org/10.1145/3605764.3623985

[3]H. Aghakhani, W. Dai, A. Manoel, X. Fernandes, A. Kharkar,
C. Kruegel, G. Vigna, D. Evans, B. Zorn, and R. Sim, “Trojanpuzzle:
Covertly poisoning code-suggestion models,” in IEEE S&P, 2024.
[Online]. Available: https://arxiv.org/abs/2301.02344

[4]S. Barke, M. B. James, and N. Polikarpova, “Grounded copilot:
How programmers interact with code-generating models,” Proc. ACM
Program. Lang., vol. 7, no. OOPSLA1, pp. 85–111, 2023. [Online].
Available: https://doi.org/10.1145/3586030

| Item | Value 1 | Value 2 |
| --- | --- | --- |
| [5]M. Bavarian, H. Jun, N. Tezak, J. Schulman, C. McLeavey, J. Tworek, and M. Chen, “Efficient training of language models to fill in the middle,” CoRR, vol. abs/2207.14255, 2022. [Online]. Available: https://arxiv.org/abs/ | 2207.14 | 255 |
| [6]N. Carlini, M. Jagielski, C. A. Choquette-Choo, D. Paleka, W. Pearce, H. Anderson, A. Terzis, K. Thomas, and F. Trame`r, “Poisoning web-scale training datasets is practical,” in IEEE S&P, 2024. [Online]. Available: https://arxiv.org/abs/ | 2302.10 | 149 |
| [7]F. Cassano, J. Gouwar, D. Nguyen, S. Nguyen, L. Phipps- Costin, D. Pinckney, M.-H. Yee, Y. Zi, C. J. Anderson, M. Q. Feldman, A. Guha, M. Greenberg, and A. Jangda, “Multipl-e: A scalable and extensible approach to benchmarking neural code generation,” CoRR, vol. abs/2208.08227, 2022. [Online]. Available: https://arxiv.org/abs/ | 2208.08 | 227 |
| [8]M. Chen, J. Tworek, H. Jun, Q. Yuan, H. P. de Oliveira Pinto, J. Kaplan, H. Edwards, Y. Burda, N. Joseph, G. Brockman et al., “Evaluating large language models trained on code,” CoRR, vol. abs/2107.03374, 2021. [Online]. Available: https://arxiv.org/abs/ | 2107.03 | 374 |

[9]Cursor, “Cursor - The AI Code Editor,” 2024. [Online]. Available:
https://www.cursor.com/

[10] T. Dohmke, “GitHub Copilot X: The AI-powered developer
experience,” 2023. [Online]. Available: https://github.blog/2023-03-
22-github-copilot-x-the-ai-powered-developer-experience/

[11] D. Fried, A. Aghajanyan, J. Lin, S. Wang, E. Wallace, F. Shi,
R. Zhong, S. Yih, L. Zettlemoyer, and M. Lewis, “Incoder: A
generative model for code infilling and synthesis,” in ICLR, 2023.
[Online]. Available: https://openreview.net/pdf?id=hQwb-lbM6EL

[12] GitHub, “GitHub Copilot - Your AI pair programmer,” 2024. [Online].
Available: https://github.com/features/copilot

[13] I. Goldman and Y. Kradkoda, “Can you trust your vscode extensions?”
1 2023. [Online]. Available: https://www.aquasec.com/blog/can-you-
trust-your-vscode-extensions/

[14] H. Hajipour, K. Hassler, T. Holz, L. Scho¨nherr, and M. Fritz,
“Codelmsec benchmark: Systematically evaluating and finding security
vulnerabilities in black-box code language models,” in SaTML, 2024.
[Online]. Available: https://openreview.net/forum?id=ElHDg4Yd3w

[15] J. He and M. Vechev, “Large language models for code: Security
hardening and adversarial testing,” in CCS, 2023. [Online]. Available:
https://doi.org/10.1145/3576915.3623175

[16] A. Hindle, E. T. Barr, Z. Su, M. Gabel, and P. T. Devanbu, “On
the naturalness of software,” in ICSE, 2012. [Online]. Available:
https://doi.org/10.1109/ICSE.2012.6227135

[17] N. Jain, A. Schwarzschild, Y. Wen, G. Somepalli, J. Kirchenbauer,
P.-y. Chiang, M. Goldblum, A. Saha, J. Geiping, and T. Goldstein,
“Baseline defenses for adversarial attacks against aligned language
models,” arXiv preprint arXiv:2309.00614, 2023.

[18] R. Khoury, A. R. Avila, J. Brunelle, and B. M. Camara, “How secure
is code generated by chatgpt?” in IEEE International Conference
on Systems, Man, and Cybernetics, SMC, 2023. [Online]. Available:
https://doi.org/10.1109/SMC53992.2023.10394237

[19] R. Li, L. B. Allal, Y. Zi, N. Muennighoff, D. Kocetkov, C. Mou,
M. Marone, C. Akiki, J. Li, J. Chim, and Others, “Starcoder: may
the source be with you!” CoRR, vol. abs/2305.06161, 2023. [Online].
Available: https://arxiv.org/abs/2305.06161

[20] E. Nijkamp, B. Pang, H. Hayashi, L. Tu, H. Wang, Y. Zhou,
S. Savarese, and C. Xiong, “Codegen: An open large language model
for code with multi-turn program synthesis,” in ICLR, 2023. [Online].
Available: https://openreview.net/pdf?id=iaYcJKpY2B

[21] Y. Nong, Y. Ou, M. Pradel, F. Chen, and H. Cai, “VULGEN: realistic
vulnerability generation via pattern mining and deep learning,” in
ICSE, 2023. [Online]. Available: https://doi.org/10.1109/ICSE48619.
2023.00211

[22] OpenAI, “Introduction - OpenAI API,” 2024. [Online]. Available:
https://platform.openai.com/docs/introduction

[23] H. Pearce, B. Ahmad, B. Tan, B. Dolan-Gavitt, and R. Karri,
“Asleep at the keyboard? assessing the security of github copilot’s
code contributions,” in IEEE S&P, 2022. [Online]. Available:
https://doi.org/10.1109/SP46214.2022.9833571

[24] N. Perry, M. Srivastava, D. Kumar, and D. Boneh, “Do users write
more insecure code with AI assistants?” in CCS, 2023. [Online].
Available: https://doi.org/10.1145/3576915.3623157

[25] V. Raychev, M. Vechev, and E. Yahav, “Code completion with
statistical language models,” in PLDI, 2014. [Online]. Available:
https://doi.org/10.1145/2594291.2594321

[26] B. Rozie`re, J. Gehring, F. Gloeckle, S. Sootla, I. Gat, X. E. Tan,
Y. Adi, J. Liu, T. Remez, J. Rapin et al., “Code llama: Open
foundation models for code,” CoRR, vol. abs/2308.12950, 2023.
[Online]. Available: https://arxiv.org/abs/2308.12950

[27] G.Sandoval,H.Pearce,T.Nys,R.Karri,S.Garg,and
B. Dolan-Gavitt, “Lost at C: A user study on the security
implications of large language model code assistants,” in USENIX
Security, 2023. [Online]. Available: https://www.usenix.org/conference/
usenixsecurity23/presentation/sandoval

[28] R. Schuster, C. Song, E. Tromer, and V. Shmatikov, “You
autocomplete me: Poisoning vulnerabilities in neural code completion,”
in USENIX Security, 2021. [Online]. Available: https://www.usenix.
org/conference/usenixsecurity21/presentation/schuster

[29] M.TabachnykandS.Nikolov,“ML-EnhancedCode
CompletionImprovesDeveloperProductivity,”2022.[On-
line]. Available: https://ai.googleblog.com/2022/07/ml-enhanced-code-
completion-improves.html

[30] P. Vaithilingam, T. Zhang, and E. L. Glassman, “Expectation vs.
experience: Evaluating the usability of code generation tools powered
by large language models,” in CHI Extended Abstracts, 2022.
[Online]. Available: https://doi.org/10.1145/3491101.3519665

[31] A. Vaswani, N. Shazeer, N. Parmar, J. Uszkoreit, L. Jones, A. N.
Gomez, L. Kaiser, and I. Polosukhin, “Attention is all you need,” in
NeurIPS, 2017. [Online]. Available: https://proceedings.neurips.cc/
paper/2017/hash/3f5ee243547dee91fbd053c1c4a845aa-Abstract.html

[32] F.Wu,X.Liu,andC.Xiao,“Deceptprompt:Exploiting
llm-drivencodegenerationviaadversarialnaturallanguage
instructions,” CoRR, vol. abs/2312.04730, 2023. [Online]. Available:
https://arxiv.org/abs/2312.04730

