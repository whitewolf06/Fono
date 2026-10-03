#include "runtime.hpp"
#include <condition_variable>
#include <deque>
#include <iostream>
#include <map>
#include <mutex>

struct Job { json request; std::shared_ptr<std::atomic_bool> cancelled; };
struct Control {
    std::mutex mutex, output_mutex;
    std::condition_variable changed;
    std::deque<Job> queue;
    std::map<std::string, std::shared_ptr<std::atomic_bool>> cancellations;
    bool closed = false;
};
void output(const std::shared_ptr<Control> & control, const json & response) {
    std::lock_guard<std::mutex> lock(control->output_mutex);
    const auto serialized = response.dump();
    if (serialized.size() > MAX_RESPONSE_FRAME_BYTES) {
        std::cout << worker_error(response.value("request_id", "unparsed"), "response_frame", "response exceeds limit").dump() << '\n' << std::flush;
    } else { std::cout << serialized << '\n' << std::flush; }
}
bool limited_line(std::string & line, bool & exceeded) {
    line.clear(); exceeded = false;
    char character;
    bool consumed = false;
    while (std::cin.get(character)) {
        consumed = true;
        if (character == '\n') { break; }
        if (line.size() == MAX_REQUEST_FRAME_BYTES) { exceeded = true; }
        if (!exceeded) { line.push_back(character); }
    }
    if (!line.empty() && line.back() == '\r') { line.pop_back(); }
    return consumed;
}
void reader(const std::shared_ptr<Control> & control) {
    std::string line; bool exceeded;
    while (limited_line(line, exceeded)) {
        try {
            if (exceeded) { output(control, worker_error("unparsed", "request_frame", "request exceeds limit")); continue; }
            auto request = json::parse(line);
            const auto id = request.value("request_id", "unparsed");
            if (request.value("protocol_version", 0) != PROTOCOL_VERSION) { output(control, worker_error(id, "protocol_version", "unsupported protocol version")); continue; }
            std::unique_lock<std::mutex> lock(control->mutex);
            if (request.value("type", "") == "cancel_request") {
                const auto target = request.value("target_request_id", "");
                const auto found = control->cancellations.find(target);
                if (found != control->cancellations.end()) { found->second->store(true); }
                continue;
            }
            if (control->closed) { break; }
            if (control->queue.size() >= 2 || control->cancellations.count(id)) {
                lock.unlock(); output(control, worker_error(id, "busy", "worker queue is full or request id is duplicate")); continue;
            }
            auto cancelled = std::make_shared<std::atomic_bool>(false);
            control->cancellations[id] = cancelled;
            control->queue.push_back({std::move(request), std::move(cancelled)});
            lock.unlock(); control->changed.notify_one();
        } catch (const std::exception & error) { output(control, worker_error("unparsed", "request_json", error.what())); }
    }
    std::lock_guard<std::mutex> lock(control->mutex);
    control->closed = true;
    for (const auto & entry : control->cancellations) { entry.second->store(true); }
    control->changed.notify_all();
}
int main() {
    std::ios::sync_with_stdio(false);
    auto control = std::make_shared<Control>();
    std::thread(reader, control).detach();
    std::optional<LoadedModel> loaded;
    while (true) {
        Job job;
        {
            std::unique_lock<std::mutex> lock(control->mutex);
            control->changed.wait(lock, [&] { return control->closed || !control->queue.empty(); });
            if (control->queue.empty()) { break; }
            job = std::move(control->queue.front()); control->queue.pop_front();
        }
        bool shutdown = false;
        try {
            auto response = handle(job.request, loaded, *job.cancelled);
            output(control, response.first); shutdown = response.second;
        } catch (const std::exception & error) {
            output(control, worker_error(job.request.value("request_id", "unparsed"), "transcribe", error.what()));
        }
        {
            std::lock_guard<std::mutex> lock(control->mutex);
            control->cancellations.erase(job.request.value("request_id", "unparsed"));
            if (shutdown) { control->closed = true; }
        }
        if (shutdown) { break; }
    }
    return 0;
}