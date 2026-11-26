# Prepare Local AI Install Prompts for Constrained GPUs

## Budget the VRAM before you write the prompt

- Add the GGUF file size, the mmproj size when vision is included, the KV and context cost, the compute buffer and the graphical session cost before you claim a fit.

- Leave a free margin under the card total and state the arithmetic in the prompt so the installer can check it against the load log.

- Treat a Pascal card as having no Flash Attention and no useful FP8 or FP4 acceleration, and budget the attention buffer without Flash Attention for contexts of 8192 tokens or more.

- Prefer experts in RAM with the dense parts on the GPU for a MoE model on a small card, because keeping the KV cache on an 8 GB Pascal GPU can overflow from the attention buffer alone.

- Start --n-cpu-moe from a scaled estimate for the card and the model, then lower it when generation is slow and raise it only when the load runs out of memory.

- Never tell the installer to raise --n-cpu-moe to fix slowness, because that moves more experts to the CPU and makes generation slower.

## Choose the build and the weights for the card

- Check the model card for the exact architecture name and confirm the llama.cpp build supports that architecture before you pin a build number.

- Use the GGUF variant that the build supports for that architecture and report the exact file name and size of every file the installer must download.

- Match the CUDA build to what the card supports and avoid CUDA builds that dropped the card architecture.

- Choose a quantization the card can run at full speed and avoid FP8 builds on Pascal, where they run emulated instead of accelerated.

- Keep the same software stack when you compare a current card with a planned upgrade, so the prompt still works after the upgrade changes only the memory margin.

## Write the prompt with the checks that prove the fit

- Separate text tests from the vision test, run the text tests without the mmproj at the large context and run the vision test with the mmproj at a smaller context.

- Ask the installer to confirm in the log how many layers loaded on the GPU and to report VRAM and RAM peaks beside the token speeds.

- Disable thinking on brief factual and code tests when the model thinks by default, so a short test does not spend minutes on internal reasoning.

- Try pinning threads to the physical cores and compare it with the default threading in the same run, then keep the faster setting in the final command.

- Ask for prompt and generation speeds from a benchmark in the run and label any speed you estimated from another card as an estimate, never as a measurement.

- Stop one local server before you start the other when both need the same small VRAM, and keep their folders and ports separate so they can coexist while installed.

## Recover the first load and report honestly

- If the first load shows layers silently moved to the CPU, lower the context or split the vision test before you change the model or the quantization.

- If a ComfyUI canvas opens without a save node, drag a generated PNG from the output folder onto the canvas to rebuild the workflow, then save it under a clear name.

- Report the final command, the measured speeds, the memory peaks and the quality check together, and say plainly which numbers are measured on the user card and which are estimates.

- Name the next test only after the current server is stopped or left running on a stated port, so the user can see the single card queue.

## Available connectors

- Use `muse.connector_search` to find other connectors and `muse.skill_search` to find related skills.

- If nothing here fits, look on the web for a maintained API or MCP server and set it up with [Custom Connectors and MCPs](/opt/muse/skills/custom-connector-setup/SKILL.md), or build a custom connector.

## Websites

- [llama.cpp releases](https://github.com/ggml-org/llama.cpp/releases) for current build names and CUDA or Vulkan variants. Carry the exact asset name into the prompt and verify the architecture support note there.

- [Hugging Face](https://huggingface.co/) for GGUF file lists and exact sizes in the model repository. Carry the exact file names into the prompt and verify the mmproj file in the same repository.

## Alternatives for better results

- Split a vision test to a smaller context when the combined budget overflows, instead of dropping vision or shrinking the text test.

- Drop one quantization step when RAM is tight and keep the model on the same build, instead of switching to a build the card does not support.

- Ask the user to judge image quality from the generated files when automated checks cannot show whether the output matches what was asked.
