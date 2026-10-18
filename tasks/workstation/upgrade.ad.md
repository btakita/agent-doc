---
agent_doc_session: 010b6450-eeac-40f6-9ae9-b5e2adbddc36
agent_doc_format: template
agent_doc_write: crdt
agent: codex
resume:
  claude: 80050dae-ffc1-48f6-bb8c-17c794ecb6ac
  codex: 01a14d5c-352b-7781-b889-76815eb85c88
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: Codex session selection — gpt-5 · 2026-10-18T00:54-04:00

The document is now configured to use Codex (`agent: codex`). Its queue and backlog are empty, so there is no workstation-upgrade action to execute yet.

Session-accretion is advisory-only: two recent no-op closeouts were detected. I left the exchange history intact because compaction requires your explicit approval.

### Re: Subagent completed: `subagent:gpu_power` returned work for queue item “🚧 When I run heavy GPU load on my 5090 with my 1200W PSU, my machine hard crashes. Do I need a bigger PSU? My UPS is rates <1200W as well. I have not been able to definitively identify what the cause is.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=6a5385fee5c6ef58f70377fb1092c61b767f3670fd1e501e99f9b5c3a430b9db -->
> **Queue prompt:** 🚧 When I run heavy GPU load on my 5090 with my 1200W PSU, my machine hard crashes. Do I need a bigger PSU? My UPS is rates <1200W as well. I have not been able to definitively identify what the cause is.

### Re: RTX 5090 hard crashes under load — gpt-5 · 2026-10-18T01:03-04:00

Probably not—a larger PSU should not be the first purchase. NVIDIA rates the reference RTX 5090 at 575 W and specifies a 1000 W minimum system PSU, although an overclocked partner card or unusually power-hungry CPU can raise the requirement. A good 1200 W ATX 3.x supply is ordinarily enough; its age, quality, transient response, and cabling matter as much as the wattage printed on it. [NVIDIA RTX 5090 specifications](https://www.nvidia.com/en-gb/geforce/graphics-cards/50-series/rtx-5090/) [NVIDIA user guide](https://www.nvidia.com/content/geforce-gtx/geforce-rtx-5090-user-guide-r2.pdf)

The UPS is a leading *testable* suspect, but it is not proven. The PSU's 1200 W label is its maximum DC output, not the PC's constant wall draw. The UPS instead has hard AC watt and VA limits; if actual wall draw or a transient exceeds them, it can shut off its output. UPS guidance recommends sizing from measured load with roughly 20–30% headroom, and a strong overload can shut the UPS down. [Schneider Electric UPS sizing guide](https://www.se.com/us/en/work/support/product-support/ups-buying-guide-for-selecting-a-battery-backup-system/) [APC overload behavior](https://www.apc.com/ca/en/faqs/FA158830/)

Use this sequence before buying anything:

1. Stop repeated stress testing for now. Power down, switch off and unplug the PSU, and inspect both ends of the GPU power cable and the 16-pin socket. If you see discoloration, melting, deformation, looseness, or smell burning, stop using the system and contact the GPU/PSU vendor.
2. Reseat the connector fully and flush, without tight bending or side load. With the Founders Edition adapter, NVIDIA calls for four independent dedicated PCIe 8-pin cables; otherwise follow the exact card manufacturer's manual. Never mix modular PSU cables from different models.
3. Check the UPS display/software/event log immediately after a crash for overload, output-off, transfer, or abnormal input voltage. Record peak watts and load percentage, and remove nonessential devices from battery-backed outlets.
4. With the PC shut down first, test it from a known-good grounded wall outlet on a suitable circuit—no daisy chain or questionable extension. If the crash disappears, the UPS or upstream power path is implicated. Choose any replacement by both its watt and VA ratings, measured peak wall draw plus headroom, and preferably pure-sine output for an active-PFC PSU.
5. If it still crashes from the wall, return CPU, GPU, and RAM settings to stock for diagnosis, then try a 70–80% GPU power limit. Improved stability implicates the power path but does not by itself prove which component is faulty.
6. Check the exact PSU model, age, ATX version, native 12V-2x6 cable, GPU model, CPU, and card vendor's PSU recommendation. Before buying 1500–1600 W blindly, test a known-good, high-quality 1200 W-or-larger ATX 3.x PSU with its own native cable.
7. If power remains on while only video fails, investigate the GPU/driver, temperatures, PCIe seating, and CPU/RAM stability. Windows Kernel-Power 41 only confirms an unclean shutdown; it does not identify the cause.

A useful symptom split: UPS alarm or loss of all UPS-backed outputs points toward the UPS; the PC alone instantly powering off and perhaps needing PSU cycling points toward PSU protection/cabling; a black screen with fans and system still running points more toward GPU/driver behavior.

For a precise recommendation, provide the PSU make/model/age, UPS make/model with both VA and W ratings, exact 5090 and CPU models, overclock settings, UPS alarms/logs, and whether the entire UPS output or only the PC dies.

Session accretion is elevated, but I kept the active queue draining rather than interrupting it for compaction.

### Re: Subagent completed: `subagent:circuit_limit` returned work for queue item “🚧 The room I'm in runs on a 15A circuit. I have tripped the circuit breaker at times. Is there a way to avoid this? Perhaps a batter that stores energy that can have higher power draw in bursts?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=eced2b08f06e20b8a5f39b1bf7930189fac1d03051634c6a0f001fd7429a8201 -->
> **Queue prompt:** 🚧 The room I'm in runs on a 15A circuit. I have tripped the circuit breaker at times. Is there a way to avoid this? Perhaps a batter that stores energy that can have higher power draw in bursts?

### Re: 15 A circuit and battery buffering — gpt-5 · 2026-10-18T01:05-04:00

A normal desktop UPS will not prevent branch-circuit overload. While utility power is available, it draws the computer's power from that same 15 A circuit and adds conversion losses and, after an outage, battery-recharging demand. Its battery normally takes over when input power fails; it does not blend battery power into utility power merely to cover GPU bursts.

Specialized commercial UPS/BESS equipment can provide *peak shaving* or an input-current limit, but only when that feature is explicitly documented and the complete system is properly sized, listed, and installed. It is not safe to assume a consumer UPS or portable power station does this, and deliberately cycling a normal UPS onto battery only postpones the energy draw until recharge while aging the battery.

At 120 V, a 15 A circuit has a nominal 1,800 VA limit. For a load expected to run at maximum for three hours or more, NEC-style branch sizing uses 125% of the continuous load—effectively about 12 A or 1,440 VA on an ordinary 15 A branch. That is VA rather than guaranteed usable PC watts, and every light, display, heater, printer, or other receptacle on the breaker shares it. [NFPA NEC development material](https://docinfofiles.nfpa.org/files/AboutTheCodes/70/70_A2022_NEC_P18_FD_PIReport_rev.pdf)

Repeated breaker trips are a safety signal, not normal GPU behavior. Do not repeatedly reset and stress it. If an outlet, plug, receptacle, or cable is hot, loose, discolored, buzzing, or smells burnt, stop using it and have a licensed electrician inspect it. Consumer electrical-safety guidance recommends investigation of frequent trips rather than simply resetting the breaker. [Electrical Safety Foundation guidance](https://www.esfi.org/fuse-and-breaker-breakdown/)

Practical options, in order:

1. Identify everything sharing that breaker; another room or receptacle may still be on the same branch. Move high-draw devices such as heaters, air conditioners, laser printers, and vacuums off it. Verify the destination is genuinely another circuit.
2. Measure actual UPS input/load watts, VA, and current during the workload and during battery recharge. UPS telemetry is useful; an electrician can verify the entire branch and its receptacles/connections.
3. As a temporary mitigation, reduce the RTX 5090 power limit and CPU package-power limit/Eco mode, and avoid simultaneous high-draw appliances. This reduces demand but does not repair a wiring or breaker problem.
4. Have a licensed electrician assess a dedicated circuit—often 20 A/120 V, or a compatible 240 V circuit for equipment designed for it. Do **not** replace the 15 A breaker with a 20 A breaker unless the electrician verifies the entire wiring and receptacle path is rated for it.
5. Select the UPS only after the circuit plan: correct input plug and voltage, pure-sine output, and both VA and watt capacity above measured demand with headroom. APC recommends UPS watt capacity about 20–25% above attached load. [APC sizing guidance](https://www.apc.com/us/en/support/product-support/ups-buying-guide-for-selecting-a-battery-backup-system.jsp)

For the planned RTX 5090 plus RTX PRO 6000 system, the estimated 1.4–1.5 kW DC component load becomes still higher at the wall after PSU and UPS losses. A shared 15 A/120 V branch is not an appropriate sustained-compute setup; a dedicated higher-capacity circuit, likely 240 V if the selected PSU/UPS supports it, should be part of the upgrade.

If the *panel breaker* opens, investigate the branch load/wiring first. If the breaker stays on but only the PC crashes, continue diagnosing the UPS output limit, GPU connector, PSU protection, thermals, and system stability separately.

### Re: Subagent completed: `subagent:heater_load` returned work for queue item “My GPU does not cause the circuit to trip. It is caused when I'm also running an electric heater.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=da88ea7045a4d3951936637759c268e62f92dd2c0f002ad84329e4f765e5ba47 -->

### Re: Subagent completed: `subagent:load_manager` returned work for queue item “Is there a device that can act as a circuit breaker and load manager so my circuit does not trip? Can it decrease power to a heater if my computer needs the power?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=c86702501d48925bbb6d5d834a033dba238877b2779647ac165944a849d33acd -->
> **Queue prompt:** My GPU does not cause the circuit to trip. It is caused when I'm also running an electric heater.

> **Queue prompt:** Is there a device that can act as a circuit breaker and load manager so my circuit does not trip? Can it decrease power to a heater if my computer needs the power?

### Re: Heater overload and automatic load shedding — gpt-5 · 2026-10-18T01:12-04:00

That clarification separates two problems:

- The **heater plus PC** explains the branch-circuit trips.
- A **GPU-load hard crash while the heater is off and the panel breaker remains on** is a separate PC/UPS/power-path fault and still needs the earlier UPS-log, connector, PSU, temperature, driver, and reduced-power-limit checks.

A typical 1,500 W space heater draws about 12.5 A at 120 V. That leaves only 2.5 A—roughly 300 W—before a 15 A circuit reaches its nameplate limit. A loaded RTX 5090 workstation can readily add several hundred watts, so the combined demand can reach roughly 18–21 A or more. A thermal breaker may tolerate that briefly and then trip as it heats; that is expected protection behavior, not evidence that the GPU or 1,200 W PSU alone is defective.

The safest fix is to run the heater on a genuinely different branch circuit, verified at the panel, or have an electrician install a dedicated circuit. Plug the heater directly into a wall receptacle—never an extension cord, power strip, UPS, or ordinary smart plug. If it has a manufacturer-provided 750 W low setting, that draws about 6.25 A and may provide enough margin after measuring the branch's complete load. [U.S. Fire Administration heater guidance](https://www.usfa.fema.gov/downloads/pdf/publications/heater_fire_safety_card.pdf) [CPSC heater guidance](https://www.cpsc.gov/Newsroom/News-Releases/2023/CPSC-Warns-Consumers-to-be-Cautious-When-Using-Space-Heaters-Furnaces-and-Fireplaces-This-Winter)

A device can perform automatic **load shedding**, but it does not replace the circuit breaker. A listed, electrician-installed controller can measure current and completely disconnect a lower-priority heater circuit or controlled receptacle when the workstation needs the available capacity, then reconnect it after demand falls. It generally cannot safely or smoothly “dim” a generic plug-in heater; variable heat should come from the heater's own listed low-power/staged controls.

For an installation relied upon to prevent conductor overload, ask the electrician and local authority about equipment specifically listed as a power-control or overload-control system—not merely a Wi-Fi relay or generic energy monitor. UL distinguishes ordinary UL 916 energy-management equipment from UL 3141 power-control systems used for overload control, and the controller should fail with the heater de-energized. The normal breaker remains independent overcurrent protection. [UL energy-management guidance](https://www.ul.com/thecodeauthority/knowledge/article-130-energy-management-systems)

Practically, selective shedding also requires the heater to be on a separately controllable conductor/receptacle. A panel controller cannot selectively turn off one appliance when the heater and PC are inseparably mixed on the same branch without additional wiring. Given the planned dual-GPU workstation, paying an electrician for an appropriate dedicated workstation circuit—and keeping the heater elsewhere—is safer, simpler, and more useful than engineering automation around an undersized shared branch.

The exchange is large, but I kept the active queue draining rather than interrupting it for compaction.

### Re: Subagent completed: `subagent:load_manager` returned work for queue item “📥 I don't have that option as I'm renting. Also, the circuit breakers is not in my residence, so I have to call someone to reset it. I want to avoid this issue. We have similar issues on other areas of the house. When running a heater.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=85b5b12a378f6b36a489839739343b9ce3424b264e8039729ae7050521c8037f -->
> **Queue prompt:** 📥 I don't have that option as I'm renting. Also, the circuit breakers is not in my residence, so I have to call someone to reset it. I want to avoid this issue. We have similar issues on other areas of the house. When running a heater.

### Re: Renter-safe response to repeated heater trips — gpt-5 · 2026-10-18T01:16-04:00

Since you rent and cannot access the panel, drop the load-manager idea as a DIY solution. Any relay, controlled receptacle, panel equipment, or circuit change should be a landlord/property-manager project installed by a licensed electrician. A plug-in “smart breaker” would not safely add circuit capacity or replace the real breaker.

The fact that trips occur in several areas specifically when heaters run still points strongly to load: a typical 1,500 W heater draws about 12.5 A at 120 V, nearly consuming a 15 A circuit by itself. Different rooms and outlets may share one branch circuit, and ceramic, infrared, and oil-filled 1,500 W heaters impose essentially the same full-power electrical load.

Until management has it assessed:

1. Do not run the heater with the workstation or another substantial load on any circuit that has tripped.
2. If the heater provides a manufacturer-approved 750 W/low setting, use only that as an interim measure, plugged directly into a snug wall receptacle. Do not use an extension cord, power strip, UPS, or ordinary smart plug.
3. Turn off and unplug the heater and other loads *before* asking someone to reset the breaker.
4. Record the date, room/outlet, heater wattage and setting, and what else was operating for each trip.
5. Notify the landlord/property manager in writing that heater use causes repeated trips in multiple areas and that occupants cannot access the panel. Ask for a licensed electrician to map the circuits, measure loads and voltage, inspect receptacles/connections and breaker condition, and recommend a code-compliant remedy or adequate fixed heating.
6. If any outlet or plug becomes warm, loose, discolored, buzzing, sparking, shock-producing, or smells burnt, stop using it and request urgent electrical service.

The U.S. Fire Administration advises plugging a space heater directly into the wall and putting nothing else on the same circuit; electrical-safety guidance treats recurring trips as an overload warning that should be investigated. [USFA heater safety](https://www.usfa.fema.gov/downloads/pdf/publications/heater_fire_safety_card.pdf) [ESFI overload guidance](https://www.esfi.org/dont-overload-your-home)

If management does not respond, local tenant/housing or electrical-code authorities can explain the rules for your location. Panel-access requirements have building-management exceptions, so the panel's location alone is not enough to declare a violation without jurisdiction and building details.

A lower-watt, listed personal warming product used exactly as directed may be more practical than a 1,500 W room heater, but the durable remedy is landlord-provided electrical/heating capacity—not suppressing the breaker trip.

The exchange is large; I left it intact because compaction requires your explicit approval.
### Re: Subagent completed: `subagent:local_breaker` returned work for queue item “🚧 I'm not trying to add capacity. I'm trying to avoid tripping the breaker by having an in-room breaker. But it would be great to have load-shedding as well. Power strips have a breaker. Why can't I have an in room breaker for the circuit if I plug my devices into the breaker? Is there a power strip that I can use that has a ~15A breaker? Can I configure the powerstrip to load shed on certain outlets first?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=906ff9ee4aac4b7c0a677e402b48979f06ec991ab8bb52036803ea5b068bd414 -->
### Re: Subagent completed: `subagent:local_breaker` returned work for queue item “🚧 I'm not trying to add capacity. I'm trying to avoid tripping the breaker by having an in-room breaker. But it would be great to have load-shedding as well. Power strips have a breaker. Why can't I have an in room breaker for the circuit if I plug my devices into the breaker? Is there a power strip that I can use that has a ~15A breaker? Can I configure the powerstrip to load-shed on certain outlets first?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a0fab75f23e9d6ba5075bc743475c2da4424f7afa189b6139cf1237f6aa6cd04 -->
> **Queue prompt:** 🚧 I'm not trying to add capacity. I'm trying to avoid tripping the breaker by having an in-room breaker. But it would be great to have load-shedding as well. Power strips have a breaker. Why can't I have an in room breaker for the circuit if I plug my devices into the breaker? Is there a power strip that I can use that has a ~15A breaker? Can I configure the powerstrip to load-shed on certain outlets first?

### Re: In-room breaker and outlet-priority shedding — gpt-5 · 2026-10-18T01:24-04:00

Yes—you can buy a listed 120 V power strip or rack PDU with a locally resettable 15 A breaker. Eaton's [TLM615NC](https://www.eaton.com/us/en-us/skuPage.TLM615NC.html) and [PDU1215](https://tripplite.eaton.com/support/owners-manual/754062) are examples. That can give you a convenient local reset for overloads caused by the equipment plugged into it.

The catch is that it does **not reliably solve this heater-plus-PC scenario**:

- The PDU sees only devices plugged into it. If the workstation is on the PDU and the heater remains directly in the wall, the PDU cannot see the heater current; the building breaker still sees their sum.
- Putting the heater on the PDU would make it see the sum, but CPSC guidance says a portable heater must plug directly into a wall outlet—not a power strip or extension cord. [CPSC space-heater report](https://www.cpsc.gov/s3fs-public/pdfs/CPSC-I-15-0016.pdf)
- A 15 A PDU breaker and a 15 A panel breaker are not guaranteed to open in a particular order. Which trips first depends on their time-current curves, temperature, and manufacturing tolerance. Equal handle ratings do not establish selective coordination. [Eaton coordination guidance](https://www.eaton.com/content/dam/eaton/products/electrical-circuit-protection/fuses/technical-literature/bus-ele-br-3002-spd-2017.pdf)
- Many 15 A PDUs are rated for only 12 A continuous. A typical 1,500 W heater already draws about 12.5 A, before adding the PC.

Managed 120 V/15 A PDUs also exist. CyberPower's [PDU41001](https://www.cyberpowersystems.com/product/pdus/switched/pdu41001/) has a local overload breaker and individually switchable outlets. However, its documented behavior is manual/remote or scheduled outlet control—not a safety-rated rule that detects high current and automatically sheds outlets in priority order. Eaton products advertised with automatic shedding commonly perform it after AC input is lost to extend UPS runtime, rather than to prevent an upstream breaker trip.

You could build external software around a managed PDU's meter and API, but that would be convenience automation: network/software delays or failures mean it cannot be trusted as overload protection. It also does not solve the heater restriction.

So the practical answer is:

- A locally breakered, metered PDU is reasonable for the **workstation and ordinary peripherals** if you want local monitoring/reset capability.
- It will **not guarantee** that the inaccessible panel breaker stays set when a wall-connected heater runs.
- Safe automatic priority shedding of the heater requires a listed hardwired current sensor and contactor or controlled receptacle, installed with landlord approval by an electrician.
- Without building work, use the heater's own low-wattage setting or turn it off during high workstation load. That prevents the overload instead of racing two breakers.

The existing landlord-notification TODO remains the right path for a durable load-shedding installation. The exchange is large, but I kept the active queue draining rather than interrupting it for compaction.

### Re: Subagent completed: `subagent:oil_heater` returned work for queue item “I'll get an oil heater, which can be effective at low wattage.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=4ef7e2d13f492b7bf94953d15326b8400bb80d14a40073b0b032352d79c21d39 -->
> **Queue prompt:** I'll get an oil heater, which can be effective at low wattage.

### Re: Oil-filled heater at low wattage — gpt-5 · 2026-10-18T01:48-04:00

That can be a sensible choice for quiet, steady background heat—but only if you choose a model with a **documented fixed low-power setting**, ideally around 500–600 W, and keep it in that mode.

An oil-filled radiator is not more electrically efficient than another resistance heater at the same wattage. Resistance heating is essentially one unit of electrical energy converted into one unit of room heat; the oil simply stores heat and releases it more gradually, producing slower warm-up, steadier comfort, and delayed cooldown. [DOE resistance-heating guidance](https://betterbuildingssolutioncenter.energy.gov/sites/default/files/attachments/FINAL%20Industrial%20Electrification%20Booklet.pdf) [De'Longhi oil-radiator explanation](https://www.delonghi.com/en-us/faqs/Radiant-heater/a/17537)

At 120 V:

- 600 W is about 5 A.
- 900 W is about 7.5 A.
- 1,500 W is about 12.5 A.

The fixed wattage setting matters more than the thermostat or an “Eco” label. A thermostat reduces average energy by cycling, but whenever the heating element is on it can still draw the full selected wattage. Confirm in the manual that Low truly caps instantaneous power and that startup or power restoration does not silently return to 1,500 W.

Choose a unit with a recognized NRTL listing such as UL, ETL, or CSA; a manual-stated fixed low setting; over-temperature protection; stable construction; and controls that remain on the selected low setting. Add the heater's amperage to the workstation's **measured wall draw**, monitors, lights, and every other load sharing the branch. Even 600 W is not a guarantee if the workstation and other loads already consume the remaining capacity.

Plug the heater directly into a sound wall receptacle—never the PDU, an extension cord, UPS, smart plug, or power strip. Keep at least three feet from combustibles, use it on a flat stable surface, and turn it off when unattended or sleeping. Stop immediately if the plug, cord, receptacle, or faceplate becomes hot. [CPSC space-heater safety](https://www.cpsc.gov/Newsroom/News-Releases/2026/Keep-Warm-and-Safe-This-Winter-Tips-for-Using-Generators-Furnaces-and-Space-Heaters)

I recorded the fixed-low-wattage and safety criteria in the Todo list. The exchange remains large, but I kept the active queue draining rather than interrupting it for compaction.
<!-- agent:boundary:feb4ceea -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority subagents -->
- 📥 I'll get an oil heater, which can be effective at low wattage.
<!-- /agent:queue -->

## Todo

<!-- agent:todo priority -->
- [ ] [#9n4h] Notify the landlord/property manager in writing about repeated heater-related breaker trips and request a licensed electrical inspection plus a safe breaker-reset procedure
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Done

<!-- agent:done -->
<!-- /agent:done -->

## Notes

<!-- agent:notes -->
# Workstation upgrade context from chat

## User intent

The user plans to add an NVIDIA RTX PRO 6000 Blackwell Workstation Edition to the existing RTX 5090 workstation. Populate `tasks/workstation/upgrade.ad.md` with these verified facts, conclusions, official links, and the remaining decisions so the user can continue the discussion there.

## Verified current system

- CPU: AMD Ryzen 9 9950X3D, 16 cores / 32 threads, 170 W rated TDP.
- Memory: 186 GiB usable system RAM (nominally 192 GB installed).
- GPU: NVIDIA GeForce RTX 5090 with 32,607 MiB reported VRAM.
  - Current/default power limit: 575 W.
  - Hardware maximum power limit: 600 W.
  - PCI address: `01:00.0`.
  - MSI subsystem vendor (`1462`). Exact cooler/card model remains unknown.
- Motherboard: MSI PRO X870E-P WIFI (`MS-7E70`, product version 2.0).
- PSU: ASUS ROG Strix 1200W Platinum, fully modular, ATX 3.1.
  - Rated output: 1,200 W on the 12 V rail.
  - Cabling: one native PCIe 16-pin cable plus four PCIe 6+2-pin cables.
  - Official specification: https://rog.asus.com/us/power-supply-units/rog-strix/rog-strix-1200p-gaming/spec/

## Proposed second GPU

The intended card is the 96 GB RTX PRO 6000 Blackwell Workstation Edition, not the older 48 GB RTX 6000 Ada.

- 96 GB GDDR7 ECC.
- 600 W total board power.
- PCIe 5.0 x16 interface.
- One PCIe CEM5 16-pin power connector.
- 12.0 inches long, 5.4 inches high, dual-slot, extended-height card.
- Double-flow-through cooling.
- Official specification: https://www.nvidia.com/content/dam/en-zz/Solutions/data-center/rtx-pro-6000-blackwell-workstation-edition/workstation-blackwell-rtx-pro-6000-workstation-edition-nvidia-us-3519208-web.pdf

## Power conclusion

Approximate component budget:

- RTX 5090: 575 W.
- RTX PRO 6000 Blackwell: 600 W.
- Ryzen 9 9950X3D: 170 W rated TDP.
- Board, RAM, storage, cooling, and USB: approximately 75–150 W.
- Estimated system total: approximately 1,420–1,500 W.

The existing 1,200 W PSU is not adequate. The GPUs alone can request 1,175 W, leaving only 25 W for everything else, and the PSU has only one native 16-pin output.

A 1,600 W ATX 3.1 PSU with two native 12V-2x6 outputs is the practical minimum. It is suitable when simultaneous workloads are power-limited, especially if the 5090 is reduced from 575 W toward its locally reported 400 W minimum. With both GPUs unrestricted and CPU/platform heavily loaded, it has only about 100–180 W estimated headroom, too little for the preferred sustained-compute configuration.

Current 1,600 W candidates with two native 12V-2x6 outputs:

- MSI MEG Ai1600T PCIE5: https://www.msi.com/Power-Supply/MEG-Ai1600T-PCIE5/
- ASUS ROG Thor 1600W Titanium III: https://rog.asus.com/power-supply-units/rog-thor/rog-thor-1600t3-gaming-evo/spec/
- Seasonic PRIME TX-1600 Noctua Edition: https://seasonic.com/product/prime-tx-1600-noctua-edition/

For unrestricted simultaneous GPU and CPU workloads, prefer 1,800–2,000 W with two native 12V-2x6 cables. A 2,200 W unit such as Seasonic PRIME PX-2200 provides margin but requires 200–240 V input: https://seasonic.com/wp-content/uploads/2024/06/ATX-3.1-PRIME-PX-2200.pdf

At roughly 1.4–1.5 kW DC output, wall draw is higher after conversion losses. Plan on a dedicated 20 A / 120 V circuit or preferably 240 V after verifying the chosen PSU input specification. A shared 15 A / 120 V circuit is not a comfortable sustained-compute configuration.

## Motherboard conclusion

The current board can enumerate two GPUs but is not a good full-performance combined-GPU platform:

- `PCI_E1`: PCIe 5.0 x16 directly from the CPU.
- `PCI_E3`: full-length connector operating only at PCIe 4.0 x4 through the X870E chipset.
- Official specification: https://www.msi.com/Motherboard/PRO-X870E-P-WIFI/Specification

Consequences:

- Independent jobs, one per GPU: potentially workable. The x4 link mainly affects loading and host transfers once each workload fits within its GPU.
- One model divided across both GPUs: not recommended. PCIe 4.0 x4 is about 8 GB/s theoretical one-way and traverses the chipset, making synchronization and tensor/model-parallel transfers a likely bottleneck.
- The 32 GB and 96 GB memories remain separate; software must explicitly partition/offload. They do not automatically form a unified 128 GB pool.
- For a combined workload, the RTX PRO 6000 likely belongs in the CPU-connected top slot and the 5090 in the constrained x4 slot.
- Populate the board's supplemental `PCIE_PWR` 8-pin input, but it does not replace either GPU's dedicated 16-pin cable.

For serious model parallelism, replace the motherboard with two CPU-connected slots at PCIe 5.0 x8/x8:

- ASUS ProArt X870E-Creator WIFI: https://dlcdnets.asus.com/pub/ASUS/mb/SocketAM5/ProArt_X870E-CREATOR_WIFI/E23930_ProArt_X870E-CREATOR_WIFI_EM_WEB.pdf
  - Two CPU-connected PCIe 5.0 slots at x8/x8.
  - Respect its `M.2_2` sharing rules to retain x8/x8.
- Gigabyte X870E AORUS XTREME AI TOP: https://www.gigabyte.com/za/Motherboard/X870E-AORUS-XTREME-AI-TOP-rev-1x/sp
  - Two CPU-connected slots operating at x8/x8 when both are populated.

## Mechanical and thermal checks still required

The exact case and exact MSI RTX 5090 model remain unknown.

1. Confirm the case supports the RTX PRO card's 12-inch length and 5.4-inch extended height, including safe 12V-2x6 cable clearance.
2. Confirm the 5090's slot thickness does not cover the lower full-length slot.
3. Confirm intake space remains around both cards; together they can dissipate about 1,175 W as GPU heat.
4. Use appropriate support brackets/chassis retention.
5. Never reuse modular cables from the ASUS PSU with a replacement PSU unless explicitly listed as compatible by the new PSU manufacturer.
6. Fully seat both native 12V-2x6 connectors and avoid tight bends immediately at the plugs.

## Recommended paths

- Independent workloads plus power limiting: a high-quality 1,600 W ATX 3.1 PSU with two native 12V-2x6 outputs may suffice; test the existing motherboard with the second GPU at PCIe 4.0 x4.
- One workload spanning both GPUs: upgrade to a true CPU-connected x8/x8 motherboard and use an 1,800–2,000 W PSU with suitable mains power.
- No power limits, long sustained compute, maximum margin: use the higher-wattage path and 240 V where required or beneficial.

## Questions for the next turn

1. Will the cards run independent jobs, inference with layer offload, or tensor/model-parallel training/inference?
2. What is the case make/model and exact MSI RTX 5090 model?
3. Is the outlet on a dedicated 15 A, dedicated 20 A, or 240 V circuit?
4. Is replacing the motherboard acceptable?
5. Is power-limiting the RTX 5090 while both cards run acceptable?
<!-- /agent:notes -->
